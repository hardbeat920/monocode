//! Runs use the same conversation lifecycle as an ordinary native submission.
use super::*;
use monocode_core::automations::{self, DueAutomationRun, Service};

pub(super) type ServiceHandle = Arc<Mutex<Service>>;

pub(super) struct Running {
    run_id: String,
    submitting: bool,
    generation_before: u64,
}

pub(super) struct State {
    pub service: Option<ServiceHandle>,
    pub open_requested: Option<u64>,
    opening: bool,
    polling: bool,
    recovered: bool,
    started_before: i64,
    pending: HashSet<String>,
    running: HashMap<i64, Running>,
    reserved: HashSet<i64>,
    writes: Option<gpui::Task<()>>,
    event_retries: HashMap<String, monocode_core::inbox::Item>,
    events_polling: bool,
    retry_revision: Arc<AtomicU64>,
    pub session_automations: HashMap<i64, String>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            service: None,
            open_requested: None,
            opening: false,
            polling: false,
            recovered: false,
            started_before: now(),
            pending: HashSet::new(),
            running: HashMap::new(),
            reserved: HashSet::new(),
            writes: None,
            event_retries: HashMap::new(),
            events_polling: false,
            retry_revision: Arc::new(AtomicU64::new(0)),
            session_automations: HashMap::new(),
        }
    }
}

fn now() -> i64 {
    monocode_core::time::now_ms().min(i64::MAX as u64) as i64
}

fn service_lock(service: &ServiceHandle) -> Result<std::sync::MutexGuard<'_, Service>, String> {
    service
        .lock()
        .map_err(|_| "Automations are unavailable".into())
}

impl Shell {
    pub(super) fn initialize_automations(&mut self, cx: &mut Context<Self>) {
        if self.automation_state.opening || self.automation_state.service.is_some() {
            return;
        }
        self.automation_state.opening = true;
        let store = self.store.clone();
        let work = cx.background_executor().spawn(async move {
            let path = locked(&store)?.database_path().to_path_buf();
            let service = Service::open(&path).map(|service| Arc::new(Mutex::new(service)))?;
            let retries = locked(&store)?
                .setting("native-automation-inbox-retries")?
                .and_then(|raw| serde_json::from_str::<Vec<monocode_core::inbox::Item>>(&raw).ok())
                .unwrap_or_default();
            let sessions = locked(&store)?.automation_session_ids()?;
            Ok::<_, String>((service, retries, sessions))
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.automation_state.opening = false;
                match result {
                    Ok((service, retries, sessions)) => {
                        this.automation_state.session_automations = sessions;
                        this.automation_state.service = Some(service);
                        for item in retries {
                            this.automation_state
                                .event_retries
                                .insert(item.event_key(), item);
                        }
                        if !cfg!(test) {
                            this.ensure_background_inbox(window, cx);
                        }
                        this.poll_automations(cx);
                        if this
                            .automation_state
                            .open_requested
                            .take()
                            .is_some_and(|revision| revision == this.navigation_revision)
                        {
                            this.open_automations(window, cx);
                        } else if this.navigation_revision == 0
                            && this.active_app_view.is_none()
                            && this.settings.is_none()
                        {
                            let saved = this.workspace.scope_focus.get(&this.tab_scope()).cloned();
                            if let Some(tab @ TitleTab::App(_)) = saved {
                                this.open_title_tab(&tab, window, cx);
                            }
                        }
                    }
                    Err(error) => {
                        this.error = Some(format!("Could not open automations: {error}"));
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    pub(super) fn poll_automations(&mut self, cx: &mut Context<Self>) {
        // A rejected or deferred submission still needs a terminal receipt.
        let sessions: Vec<_> = self.automation_state.running.keys().copied().collect();
        for id in sessions {
            self.automation_session_changed(id, cx);
        }
        let Some(service) = self.automation_state.service.clone() else {
            self.initialize_automations(cx);
            return;
        };
        if self.automation_state.polling {
            return;
        }
        self.automation_state.polling = true;
        let recover = !self.automation_state.recovered;
        let cutoff = self.automation_state.started_before;
        let work = cx.background_executor().spawn(async move {
            let mut service = service_lock(&service)?;
            let mut runs = if recover {
                service.recover(cutoff, now())?
            } else {
                Vec::new()
            };
            runs.extend(service.claim_due(now())?);
            let has_events = service.list()?.iter().any(|automation| {
                automation.enabled
                    && automation.triggers.as_ref().is_some_and(|triggers| {
                        triggers.iter().any(|trigger| trigger.kind != "time")
                    })
            });
            Ok::<_, String>((runs, has_events))
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.automation_state.polling = false;
                match result {
                    Ok((runs, has_events)) => {
                        this.automation_state.recovered = true;
                        for run in runs {
                            this.launch_automation(run, false, cx);
                        }
                        if has_events {
                            this.ensure_background_inbox(window, cx);
                        }
                    }
                    Err(error) => {
                        this.error = Some(format!("Could not evaluate automations: {error}"));
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    pub(super) fn inbox_automation_events(
        &mut self,
        items: Vec<monocode_core::inbox::Item>,
        cx: &mut Context<Self>,
    ) {
        for item in items {
            self.automation_state
                .event_retries
                .insert(item.event_key(), item);
        }
        if self.automation_state.event_retries.len() > 500 {
            let mut items = self
                .automation_state
                .event_retries
                .values()
                .cloned()
                .collect::<Vec<_>>();
            items.sort_by_key(|item| std::cmp::Reverse(item.updated_ms()));
            self.automation_state.event_retries = items
                .into_iter()
                .take(500)
                .map(|item| (item.event_key(), item))
                .collect();
        }
        if self.automation_state.events_polling || self.automation_state.event_retries.is_empty() {
            return;
        }
        let Some(service) = self.automation_state.service.clone() else {
            return;
        };
        self.automation_state.events_polling = true;
        let items = self
            .automation_state
            .event_retries
            .values()
            .cloned()
            .collect::<Vec<_>>();
        let store = self.store.clone();
        let attempted = items.clone();
        let revision = self
            .automation_state
            .retry_revision
            .fetch_add(1, Ordering::SeqCst)
            + 1;
        let generation = self.automation_state.retry_revision.clone();
        let work = cx.background_executor().spawn(async move {
            // Persist before claiming so a transient failure can retry after a restart.
            let saved = serde_json::to_string(&items).map_err(|error| error.to_string())?;
            {
                let store = locked(&store)?;
                if generation.load(Ordering::SeqCst) == revision {
                    store.save_setting("native-automation-inbox-retries", &saved)?;
                }
            }
            let mut service = service_lock(&service)?;
            automations::claim_inbox_events_retry(&mut service, &items, now())
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.automation_state.events_polling = false;
                match result {
                    Ok((runs, retry)) => {
                        for item in attempted {
                            this.automation_state
                                .event_retries
                                .remove(&item.event_key());
                        }
                        for item in retry {
                            this.automation_state
                                .event_retries
                                .insert(item.event_key(), item);
                        }
                        for run in runs {
                            this.launch_automation(run, false, cx);
                        }
                        let store = this.store.clone();
                        let pending = this
                            .automation_state
                            .event_retries
                            .values()
                            .cloned()
                            .collect::<Vec<_>>();
                        let revision = this
                            .automation_state
                            .retry_revision
                            .fetch_add(1, Ordering::SeqCst)
                            + 1;
                        let generation = this.automation_state.retry_revision.clone();
                        cx.background_executor()
                            .spawn(async move {
                                if let Ok(saved) = serde_json::to_string(&pending) {
                                    let _ = locked(&store).and_then(|store| {
                                        if generation.load(Ordering::SeqCst) == revision {
                                            store.save_setting(
                                                "native-automation-inbox-retries",
                                                &saved,
                                            )
                                        } else {
                                            Ok(())
                                        }
                                    });
                                }
                            })
                            .detach();
                    }
                    Err(error) => {
                        this.error = Some(format!("Could not evaluate Inbox automations: {error}"));
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    pub(super) fn launch_automation(
        &mut self,
        due: DueAutomationRun,
        reveal: bool,
        cx: &mut Context<Self>,
    ) {
        if self.automation_state.pending.contains(&due.run.id)
            || self
                .automation_state
                .running
                .values()
                .any(|run| run.run_id == due.run.id)
        {
            return;
        }
        self.automation_state.pending.insert(due.run.id.clone());
        let automation = &due.automation;
        let kind = Kind::from_id(&automation.harness);
        let reuse = automation
            .reuse_session
            .then_some(automation.last_session_id.as_deref())
            .flatten()
            .and_then(|id| locked(&self.store).ok()?.resolve_existing_session_id(id))
            .filter(|id| {
                !self.automation_state.running.contains_key(id)
                    && !self.automation_state.reserved.contains(id)
            })
            .filter(|id| {
                self.session_summaries.get(id).is_some_and(|summary| {
                    summary.provider == automation.harness
                        && !summary.archived
                        && !self.draft_sessions.contains(id)
                        && self
                            .sessions
                            .get(id)
                            .is_none_or(|view| view.read(cx).available_for_managed_run(cx))
                        && match automation.workspace_mode.as_str() {
                            "current" => {
                                self.session_worktrees
                                    .get(id)
                                    .and_then(Option::as_deref)
                                    .is_none()
                                    && monocode_core::files::path_key(&summary.cwd)
                                        == monocode_core::files::path_key(&automation.cwd)
                            }
                            "existing" => self
                                .session_worktrees
                                .get(id)
                                .and_then(Option::as_deref)
                                .is_some_and(|cwd| {
                                    monocode_core::files::path_key(cwd)
                                        == monocode_core::files::path_key(&automation.worktree_cwd)
                                }),
                            _ => false,
                        }
                })
            });
        if let Some(id) = reuse {
            self.ensure_session(id, cx);
        }
        let account = crate::accounts::get(cx)
            .map(|accounts| accounts.read(cx).selected(kind, &automation.cwd))
            .unwrap_or_default();
        if let Some(id) = reuse {
            self.automation_state.reserved.insert(id);
        }
        let mut settings = reuse
            .and_then(|id| {
                self.sessions
                    .get(&id)
                    .map(|view| view.read(cx).settings.clone())
            })
            .unwrap_or_default();
        settings.account = account;
        settings.model = monocode_core::store::native_model(&automation.model, kind.id());
        settings.model_settings = automation.model_settings.clone();
        settings.effort = [
            "effort",
            "reasoning",
            "reasoningEffort",
            "reasoning_effort",
            "variant",
        ]
        .into_iter()
        .find_map(|key| automation.model_settings.get(key).cloned());
        settings.fast = automation
            .model_settings
            .get("fast")
            .is_some_and(|value| value == "true");
        settings.service_tier = automation
            .model_settings
            .get("serviceTier")
            .or_else(|| automation.model_settings.get("service_tier"))
            .cloned();
        settings.thinking = automation
            .model_settings
            .get("thinking")
            .and_then(|value| value.parse().ok());
        settings.access = monocode_core::agent::AccessMode::from_id(&automation.runtime_mode);
        settings.new_worktree = automation.workspace_mode == "worktree";
        settings.worktree_base = (automation.workspace_mode == "worktree").then(|| "HEAD".into());
        settings.worktree_cwd =
            (automation.workspace_mode == "existing").then(|| automation.worktree_cwd.clone());
        settings.plan = false;
        settings.operator = false;
        settings.orchestrator = false;
        settings.prepared_prompt = None;
        let store = self.store.clone();
        let launch = due.clone();
        let revision = self.navigation_revision;
        let work = cx.background_executor().spawn(async move {
            if Kind::parse(&launch.automation.harness).is_none() {
                return Err("The automation's agent is unavailable".into());
            }
            if !std::path::Path::new(&launch.automation.cwd).is_dir() {
                return Err("The automation's project folder is unavailable".into());
            }
            if launch.automation.workspace_mode == "existing"
                && !std::path::Path::new(&launch.automation.worktree_cwd).is_dir()
            {
                return Err("The automation's worktree is unavailable".into());
            }
            let store = locked(&store)?;
            let mut settings = settings;
            let mut summary = if let Some(id) = reuse {
                if let Some(previous) = store.agent_settings(id)? {
                    settings.turns = previous.turns;
                    settings.revision = previous.revision;
                }
                store
                    .sessions()?
                    .into_iter()
                    .find(|session| session.id == id)
                    .ok_or("The reusable session no longer exists")?
            } else {
                let mut summary =
                    store.create(&launch.automation.harness, &launch.automation.cwd)?;
                if launch.run.trigger != "event" {
                    summary.title = launch.automation.name.clone();
                    store.title(summary.id, &summary.title)?;
                }
                summary
            };
            store.set_automation_id(summary.id, &launch.automation.id)?;
            if let Some(item) = automation_work_item(&launch.run) {
                store.set_work_item(summary.id, Some(&item))?;
                summary.work_item = Some(item);
            }
            if reuse.is_none() {
                store.save_agent_settings(summary.id, &settings)?;
            }
            let persisted = store.persisted_session_id(summary.id)?;
            Ok::<_, String>((summary, settings, persisted))
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.automation_state.pending.remove(&due.run.id);
                if let Some(id) = reuse {
                    this.automation_state.reserved.remove(&id);
                }
                match result {
                    Ok((summary, settings, persisted)) => {
                        let id = summary.id;
                        if reuse.is_some()
                            && this
                                .sessions
                                .get(&id)
                                .is_none_or(|view| !view.read(cx).available_for_managed_run(cx))
                        {
                            let mut retry = due.clone();
                            retry.automation.reuse_session = false;
                            this.launch_automation(retry, reveal, cx);
                            return;
                        }
                        let linked = summary.work_item.clone();
                        this.add(summary, cx);
                        if let Some(linked) = linked {
                            this.sessions[&id]
                                .update(cx, |view, _| view.summary.work_item = Some(linked));
                        }
                        this.automation_state
                            .session_automations
                            .insert(id, due.automation.id.clone());
                        this.automation_state.running.insert(
                            id,
                            Running {
                                run_id: due.run.id.clone(),
                                submitting: true,
                                generation_before: this.sessions[&id]
                                    .read(cx)
                                    .submission_generation,
                            },
                        );
                        this.update_automation_run(
                            due.run.id.clone(),
                            "running",
                            Some(persisted),
                            None,
                            cx,
                        );
                        if !due.automation.session_folder_id.is_empty() {
                            this.groups
                                .entry(due.automation.cwd.clone())
                                .or_default()
                                .add_to_folder(&due.automation.session_folder_id, id);
                            this.persist_automation_folder(&due.automation.cwd, cx);
                        }
                        // Background runs append a tab without changing the focused project.
                        if !this.workspace.tabs.contains(&id) {
                            this.workspace.tabs.push(id);
                            this.workspace.title_order.push(TitleTab::Session(id));
                        }
                        let prompt = due
                            .run
                            .prompt
                            .clone()
                            .unwrap_or_else(|| due.automation.prompt.clone());
                        this.sessions[&id]
                            .update(cx, |view, cx| view.quick_submit(prompt, settings, cx));
                        if let Some(run) = this.automation_state.running.get_mut(&id) {
                            run.submitting = false;
                        }
                        this.automation_session_changed(id, cx);
                        if reveal && this.navigation_revision == revision {
                            this.activate_session(id, window, cx);
                        }
                        this.persist(cx);
                    }
                    Err(error) => {
                        this.update_automation_run(
                            due.run.id.clone(),
                            "failed",
                            None,
                            Some(error.clone()),
                            cx,
                        );
                        if reveal {
                            this.error = Some(error);
                            cx.notify();
                        }
                    }
                }
            });
        })
        .detach();
    }

    pub(super) fn automation_session_changed(&mut self, id: i64, cx: &mut Context<Self>) {
        let Some(run) = self.automation_state.running.get(&id) else {
            return;
        };
        if run.submitting {
            return;
        }
        let Some(view) = self.sessions.get(&id) else {
            return;
        };
        let view = view.read(cx);
        if view.external_submission_pending() {
            return;
        }
        let cancelled = view.notification_was_cancelled();
        let (mut ok, mut message) = view.external_outcome(cx);
        if view.submission_generation == run.generation_before {
            ok = false;
            message = "The selected agent session could not start this run.".into();
        }
        let status = if cancelled {
            "cancelled"
        } else if ok {
            "succeeded"
        } else {
            "failed"
        };
        let run = self.automation_state.running.remove(&id).unwrap();
        self.update_automation_run(
            run.run_id,
            status,
            None,
            (!ok && !cancelled).then_some(message),
            cx,
        );
    }

    fn update_automation_run(
        &mut self,
        run_id: String,
        status: &str,
        session: Option<String>,
        error: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(service) = self.automation_state.service.clone() else {
            return;
        };
        let previous = self.automation_state.writes.take();
        let status = status.to_string();
        self.automation_state.writes = Some(cx.spawn(async move |this, cx| {
            if let Some(previous) = previous {
                previous.await;
            }
            let work = cx.background_executor().spawn(async move {
                service_lock(&service)?.update_run(&run_id, &status, session, error, now())
            });
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                if let Err(error) = result {
                    this.error = Some(format!("Could not save automation result: {error}"));
                }
                if let Some(view) = &this.automations {
                    view.update(cx, |view, cx| view.refresh(cx));
                }
                cx.notify();
            });
        }));
    }

    fn persist_automation_folder(&mut self, cwd: &str, cx: &mut Context<Self>) {
        let cwd = cwd.to_string();
        let groups = self.groups.get(&cwd).cloned().unwrap_or_default();
        let store = self.store.clone();
        let previous = self.group_writes.take();
        self.group_writes = Some(cx.spawn(async move |this, cx| {
            if let Some(previous) = previous {
                previous.await;
            }
            let work = cx
                .background_executor()
                .spawn(async move { locked(&store)?.save_groups(&cwd, &groups) });
            if let Err(error) = work.await {
                let _ = this.update(cx, |this, cx| {
                    this.error = Some(error);
                    cx.notify();
                });
            }
        }));
    }
}

fn automation_work_item(
    run: &monocode_core::automations::AutomationRun,
) -> Option<monocode_core::session_groups::WorkItem> {
    if run.trigger != "event" || run.event_kind.as_deref() != Some("github") {
        return None;
    }
    let mut parts = run.event_key.as_deref()?.split(':');
    if parts.next()? != "github" {
        return None;
    }
    let kind = match parts.next()? {
        "pr" => "pull",
        "issue" => "issues",
        _ => return None,
    };
    let repo = parts.next()?;
    let number = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    monocode_core::session_groups::parse_github_work_item(&format!(
        "https://github.com/{repo}/{kind}/{number}"
    ))
}
