//! Operator's project actions. Monos share the launch/request helpers but keep
//! their own project grants, visibility preference and completion bookkeeping.
use super::*;
use monocode_core::{
    agent::Settings,
    app_access as app,
    mono::bridge::{Call, Server},
};
use serde_json::{Value, json};

#[cfg(test)]
mod tests;

type Placement = Option<(i64, Edge)>;
enum Effect {
    None,
    Start(Box<(Session, Settings, bool, bool, Placement)>),
    Groups(String, monocode_core::session_groups::Groups),
    Archive(i64),
}
impl Shell {
    pub(super) fn init_app_bridge(&mut self, cx: &mut Context<Self>) {
        if cx.is_test() {
            return;
        }
        match Server::start() {
            Ok((server, calls)) => {
                self.app_bridge = Some(server);
                cx.spawn(async move |this, cx| {
                    while let Ok(call) = calls.recv().await {
                        if this
                            .update_in(cx, |this, window, cx| {
                                this.handle_app_call(call, window, cx)
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                })
                .detach();
            }
            Err(error) => self.error = Some(format!("Operator app access: {error}")),
        }
    }
    fn finish_app_call(
        &mut self,
        call: Call,
        result: Result<Value, String>,
        cx: &mut Context<Self>,
    ) {
        if !mutating(&call.action) {
            self.app_pending
                .remove(&(call.session, call.request_id.clone()));
            let _ = call.reply.send(result);
            return;
        }
        let fingerprint = app::fingerprint(&call.action, &call.input);
        if self.app_results.len() >= 256
            && let Some(key) = self.app_results.keys().next().cloned()
        {
            self.app_results.remove(&key);
        }
        self.app_results.insert(
            (call.session, call.request_id.clone()),
            (fingerprint.clone(), result.clone()),
        );
        let store = self.store.clone();
        let key = app::receipt_key(call.session, &call.request_id);
        let receipt = json!([fingerprint, result]).to_string();
        // A successful reply is sent only after its receipt reaches durable storage.
        let saved = cx
            .background_executor()
            .spawn(async move { locked(&store)?.save_setting(&key, &receipt) });
        cx.spawn(async move |this, cx| {
            let saved = saved.await;
            let _ = this.update(cx, |this, _| {
                this.app_pending
                    .remove(&(call.session, call.request_id.clone()));
            });
            let _ = call.reply.send(saved.and(result));
        })
        .detach();
    }
    fn app_background<F>(&mut self, call: Call, work: F, cx: &mut Context<Self>)
    where
        F: FnOnce(Database) -> Result<(Value, Effect), String> + Send + 'static,
    {
        let store = self.store.clone();
        let work = cx.background_executor().spawn(async move { work(store) });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update_in(cx, |this, window, cx| {
                let result = result.map(|(value, effect)| {
                    match effect {
                        Effect::None => (),
                        Effect::Start(start) => {
                            let (summary, settings, draft, reveal, placement) = *start;
                            let id = summary.id;
                            this.draft_sessions.insert(id);
                            this.sidebar_hidden_sessions.remove(&id);
                            this.session_worktrees
                                .insert(id, settings.worktree_cwd.clone());
                            this.add(summary, cx);
                            this.started_by.insert(id, call.session);
                            if let Some(view) = this.sessions.get(&id) {
                                view.update(cx, |v, cx| v.install_app_launch(settings, !draft, cx));
                            }
                            this.app_place_launch(id, placement, reveal, window, cx);
                        }
                        Effect::Groups(project, groups) => {
                            this.groups.insert(project, groups);
                        }
                        Effect::Archive(id) => {
                            if let Some(summary) = this.session_summaries.get_mut(&id) {
                                summary.archived = true;
                            }
                            if let Some(view) = this.sessions.get(&id) {
                                view.update(cx, |v, _| v.summary.archived = true);
                            }
                            this.refresh_session_mark(id, cx);
                            if this.workspace.tab_of(id).is_some() {
                                this.close_pane(id, window, cx);
                            }
                        }
                    }
                    this.invalidate_chrome(cx);
                    cx.notify();
                    value
                });
                this.finish_app_call(call, result, cx);
            });
        })
        .detach();
    }
    fn authorize_app_call(&self, call: &Call, cx: &App) -> Result<(), String> {
        app::validate_operator(&call.action, &call.input)?;
        if call.request_id.is_empty()
            || call.request_id.len() > 128
            || !call
                .request_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err("request ID must use letters, digits, underscores or hyphens".into());
        }
        let caller = self
            .sessions
            .get(&call.session)
            .map(|v| v.read(cx))
            .filter(|v| v.settings.operator_enabled)
            .ok_or("Operator is not enabled for this thread.")?;
        if caller.summary.cwd.starts_with("remote://") {
            return Err("Operator is not available on remote projects".into());
        }
        if call
            .input
            .get("project")
            .and_then(Value::as_str)
            .is_some_and(|p| {
                monocode_core::files::path_key(p)
                    != monocode_core::files::path_key(&caller.summary.cwd)
            })
        {
            return Err("Operator can only access this thread's project.".into());
        }
        if (caller.app_read_only
            || caller.settings.plan
            || caller.settings.access == monocode_core::agent::AccessMode::ReadOnly)
            && !matches!(
                call.action.as_str(),
                "models.list"
                    | "sessions.list"
                    | "sessions.read"
                    | "folders.list"
                    | "notes.list"
                    | "notes.read"
                    | "worktrees.list"
            )
        {
            return Err("This thread's access policy allows read-only app actions.".into());
        }
        Ok(())
    }
    fn handle_app_call(&mut self, call: Call, window: &mut Window, cx: &mut Context<Self>) {
        if let Err(error) = self.authorize_app_call(&call, cx) {
            let _ = call.reply.send(Err(error));
            return;
        }
        let key = (call.session, call.request_id.clone());
        let fingerprint = app::fingerprint(&call.action, &call.input);
        if let Some((hash, result)) = self.app_results.get(&key) {
            let _ = call.reply.send(if *hash == fingerprint {
                result.clone()
            } else {
                Err("Request ID was already used with different input".into())
            });
            return;
        }
        if !self.app_pending.insert(key) {
            let _ = call.reply.send(Err(
                "Request is still running; retry with the same request ID.".into(),
            ));
            return;
        }
        if !mutating(&call.action) {
            self.dispatch_app_call(call, window, cx);
            return;
        }
        let receipt_key = app::receipt_key(call.session, &call.request_id);
        let store = self.store.clone();
        let work = cx.background_executor().spawn(async move {
            locked(&store)?
                .setting(&receipt_key)?
                .map(|raw| {
                    serde_json::from_str::<(String, Result<Value, String>)>(&raw)
                        .map_err(|e| e.to_string())
                })
                .transpose()
        });
        cx.spawn(async move |this, cx| {
            let saved = work.await;
            let _ = this.update_in(cx, |this, window, cx| match saved {
                Ok(Some((hash, result))) => {
                    this.app_pending
                        .remove(&(call.session, call.request_id.clone()));
                    let _ = call.reply.send(if hash == fingerprint {
                        result
                    } else {
                        Err("Request ID was already used with different input".into())
                    });
                }
                Ok(None) => this.dispatch_app_call(call, window, cx),
                Err(error) => this.finish_app_call(call, Err(error), cx),
            });
        })
        .detach();
        let _ = window;
    }
    fn app_target(
        &self,
        input: &Value,
        caller: i64,
        project: &str,
        manage: bool,
        cx: &App,
    ) -> Result<i64, String> {
        let id = session_id(input, "sessionId", &self.store)?;
        let summary = self
            .session_summaries
            .get(&id)
            .ok_or("Session was not found in this project")?;
        if monocode_core::files::path_key(&summary.cwd) != monocode_core::files::path_key(project) {
            return Err("Session is outside this project.".into());
        }
        if manage
            && (id == caller
                || self.orchestration_worker(id)
                || self
                    .monos
                    .as_ref()
                    .is_some_and(|m| m.read(cx).is_mono_session(id)))
        {
            return Err("Only other regular project sessions can be managed here".into());
        }
        Ok(id)
    }
    pub(crate) fn app_placement(
        &self,
        input: &Value,
        caller: i64,
        project: &str,
        cx: &App,
    ) -> Result<Placement, String> {
        let edge = match input["placement"].as_str().unwrap_or("tab") {
            "tab" => return Ok(None),
            "right" => Edge::Right,
            "down" => Edge::Bottom,
            _ => return Err("placement must be tab, right or down".into()),
        };
        let id = if input.get("besideSessionId").is_some() {
            session_id(input, "besideSessionId", &self.store)?
        } else {
            caller
        };
        if self.workspace.tab_of(id).is_none()
            || self.session_metadata(id, cx).summary.cwd != project
        {
            return Err("besideSessionId must be an open session pane in this project".into());
        }
        Ok(Some((id, edge)))
    }
    pub(crate) fn app_place_launch(
        &mut self,
        id: i64,
        placement: Placement,
        reveal: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some((target, edge)) =
            placement.filter(|(target, _)| self.workspace.tab_of(*target).is_some())
        {
            self.open(target, window, cx);
            let before = self.active_leaves();
            self.workspace.place(id, target, edge);
            self.begin_pane_motion(before, cx);
            self.fit_sidebar(cx);
            self.activate_session(id, window, cx);
        } else if reveal {
            self.open(id, window, cx);
        }
        self.persist(cx);
    }
    fn dispatch_app_call(&mut self, call: Call, window: &mut Window, cx: &mut Context<Self>) {
        if let Err(error) = self.authorize_app_call(&call, cx) {
            self.finish_app_call(call, Err(error), cx);
            return;
        }
        let caller = self.sessions[&call.session].read(cx);
        let project = caller.summary.cwd.clone();
        let provider = caller.summary.provider.clone();
        let inherited = caller.settings.clone();
        let input = call.input.clone();
        let action = call.action.clone();
        let result: Result<Value, String> = match action.as_str() {
            "sessions.list" => Ok(
                json!({"cwd":project,"sessions":self.order.iter().filter_map(|id|{
                let metadata=self.session_metadata(*id,cx);
                if metadata.summary.cwd!=project || self.monos.as_ref().is_some_and(|m|m.read(cx).hidden_session(*id)) || self.orchestration_worker(*id){return None;}
                let live=self.sessions.get(id).map(|v|v.read(cx));
                Some(json!({"id":id,"sessionId":id,"title":metadata.summary.title,"harness":metadata.summary.provider,"model":live.and_then(|v|v.settings.model.clone()).or_else(||self.session_details.get(id).and_then(|m|m.model.clone())),"busy":metadata.busy,"hasDraft":live.is_some_and(|v|v.has_saved_draft() || !v.input.read(cx).content.is_empty()) || self.draft_sessions.contains(id),"archived":metadata.summary.archived}))
            }).collect::<Vec<_>>()}),
            ),
            "models.list" => {
                let profiles: Vec<_> = Kind::ALL
                    .into_iter()
                    .map(|kind| {
                        let profile = if kind.id() == provider {
                            inherited.account.clone()
                        } else {
                            crate::accounts::get(cx)
                                .map(|center| center.read(cx).selected(kind, &project))
                                .unwrap_or_default()
                        };
                        (kind, profile)
                    })
                    .collect();
                self.app_background(
                    call,
                    move |_| Ok((app::models_catalog(profiles)?, Effect::None)),
                    cx,
                );
                return;
            }
            "sessions.start" => {
                let parsed = (|| {
                    let prompt = app::prompt(&input)?;
                    let kind = Kind::parse(input["harness"].as_str().unwrap_or(&provider))
                        .ok_or("Unknown harness")?;
                    let placement = self.app_placement(&input, call.session, &project, cx)?;
                    let mut settings = inherited;
                    if kind.id() != provider {
                        settings.account = crate::accounts::get(cx)
                            .map(|center| center.read(cx).selected(kind, &project))
                            .unwrap_or_default();
                    }
                    Ok::<_, String>((prompt, kind, placement, settings))
                })();
                let (prompt, kind, placement, settings) = match parsed {
                    Ok(p) => p,
                    Err(e) => {
                        self.finish_app_call(call, Err(e), cx);
                        return;
                    }
                };
                let caller = call.session;
                let request = call.request_id.clone();
                let fingerprint = app::fingerprint(&action, &input);
                self.app_background(call,move|store| {
                    let models=if input.get("model").is_some() || input.get("modelSettings").is_some() || input.get("effort").is_some() {
                        monocode_core::catalog::models_for(kind,&settings.account)?
                    }else{monocode_core::catalog::cached_models_for(kind,&settings.account).unwrap_or_default()};
                    let mut settings=app::launch_settings(&input,&provider,settings,kind,&models)?;
                    settings.revision=0;
                    let title=app::optional_text(&input,"title",160)?.unwrap_or_else(||prompt.chars().take(160).collect());
                    let mut store=locked(&store)?;
                    let (mut summary,fresh)=store.create_app_session(caller,&request,&fingerprint,kind.id(),&project)?;
                    if fresh {
                        if let Err(error)=app::launch_checkout(&input,&project,summary.id,&mut settings) {let _=store.delete(summary.id);return Err(error);}
                        store.title(summary.id,&title)?;summary.title=title;
                        store.configure_agent(summary.id,kind.id(),&settings)?;
                        store.save_started_link(summary.id,caller)?;
                        store.prepare_app_prompt(summary.id,&request,&prompt)?;
                        summary.has_messages=true;
                    }
                    let draft=input["draft"].as_bool().unwrap_or(false);let reveal=input["reveal"].as_bool().unwrap_or(false);
                    let value=json!({"id":summary.id,"sessionId":summary.id,"cwd":project,"harness":kind.id(),"model":settings.model,"submitted":!draft,"draft":draft,"accepted":true});
                    Ok((value,if fresh {Effect::Start(Box::new((summary,settings,draft,reveal,placement)))}else{Effect::None}))
                },cx);
                return;
            }
            "sessions.read" => match self.app_target(&input, call.session, &project, false, cx) {
                Ok(id) => {
                    let busy = self.session_metadata(id, cx).busy;
                    let draft = self
                        .sessions
                        .get(&id)
                        .is_some_and(|v| v.read(cx).has_saved_draft())
                        || self.draft_sessions.contains(&id);
                    self.app_background(
                        call,
                        move |store| {
                            Ok((
                                locked(&store)?.app_conversation(id, &input, busy, draft)?,
                                Effect::None,
                            ))
                        },
                        cx,
                    );
                    return;
                }
                Err(e) => Err(e),
            },
            "sessions.send" | "sessions.draft" | "sessions.stop" | "sessions.archive"
            | "sessions.delete" => {
                match self
                    .app_target(&input, call.session, &project, true, cx)
                    .and_then(|id| {
                        self.ensure_session(id, cx)
                            .map(|view| (id, view))
                            .ok_or("Session not found".into())
                    }) {
                    Err(e) => Err(e),
                    Ok((id, view)) => {
                        if action == "sessions.delete" {
                            self.delete_session(id, window, cx);
                            if !self.deleting.contains(&id) {
                                self.finish_app_call(
                                    call,
                                    Err(self
                                        .error
                                        .clone()
                                        .unwrap_or("Could not delete session".into())),
                                    cx,
                                );
                                return;
                            }
                            let deleting = self.group_writes.take();
                            let store = self.store.clone();
                            self.group_writes = Some(cx.spawn(async move |this, cx| {
                                if let Some(deleting) = deleting {
                                    deleting.await;
                                }
                                let result = cx
                                    .background_executor()
                                    .spawn(async move {
                                        if locked(&store)?.sessions()?.iter().any(|s| s.id == id) {
                                            Err("Could not delete session; its data was preserved"
                                                .into())
                                        } else {
                                            Ok(json!({"sessionId":id,"deleted":true}))
                                        }
                                    })
                                    .await;
                                let _ = this
                                    .update(cx, |this, cx| this.finish_app_call(call, result, cx));
                            }));
                            return;
                        } else if action == "sessions.archive" {
                            view.update(cx, |v, cx| v.stop_app_session(cx));
                            self.app_background(
                                call,
                                move |store| {
                                    locked(&store)?.set_archived(id, true)?;
                                    Ok((
                                        json!({"sessionId":id,"archived":true}),
                                        Effect::Archive(id),
                                    ))
                                },
                                cx,
                            );
                            return;
                        } else if action == "sessions.stop" {
                            view.update(cx, |v, cx| v.stop_app_session(cx));
                            Ok(json!({"sessionId":id,"stopped":true}))
                        } else {
                            view.update(cx, |v, cx| v.load(cx));
                            if !view.read(cx).ready_for_external(cx)
                                || view.read(cx).has_unsent_draft()
                            {
                                Err("Session is busy, loading, or has an unsent draft. Its draft was preserved.".into())
                            } else {
                                match app::prompt(&input) {
                                    Err(e) => Err(e),
                                    Ok(prompt) => {
                                        view.update(cx, |v, cx| {
                                            if action == "sessions.draft" {
                                                v.save_app_draft(prompt, cx)
                                            } else {
                                                v.install_transfer(
                                                    v.settings.clone(),
                                                    Some(prompt),
                                                    cx,
                                                )
                                            }
                                        });
                                        Ok(
                                            json!({"sessionId":id,"submitted":action=="sessions.send","saved":action=="sessions.draft"}),
                                        )
                                    }
                                }
                            }
                        }
                    }
                }
            }
            "notes.list" | "notes.read" | "notes.write" => {
                let caller = call.session;
                let request = call.request_id.clone();
                self.app_background(
                    call,
                    move |store| {
                        Ok((
                            app::notes_action(
                                &*locked(&store)?,
                                caller,
                                &request,
                                &action,
                                &input,
                                &[project],
                            )?,
                            Effect::None,
                        ))
                    },
                    cx,
                );
                return;
            }
            "folders.list" => Ok(
                json!({"cwd":project,"folders":self.groups.get(&project).cloned().unwrap_or_default().folders.iter().map(|f|json!({"id":f.id,"name":f.name,"sessionIds":f.session_ids})).collect::<Vec<_>>()}),
            ),
            "folders.move" => match self.app_target(&input, call.session, &project, false, cx) {
                Err(e) => Err(e),
                Ok(id) => {
                    let mut groups = self.groups.get(&project).cloned().unwrap_or_default();
                    let folder = app::optional_text(&input, "folderId", 256);
                    let name = app::optional_text(&input, "newFolderName", 100);
                    match folder.and_then(|f| name.map(|n| (f, n))) {
                        Err(e) => Err(e),
                        Ok((folder, name)) => {
                            if folder.is_some() == name.is_some() {
                                Err("Supply exactly one of folderId or newFolderName".into())
                            } else {
                                let folder = if let Some(name) = name {
                                    let folder = groups
                                        .create_folder(&[id], monocode_core::mono::new_id())
                                        .unwrap();
                                    groups.rename_folder(&folder, &name);
                                    folder
                                } else {
                                    folder.unwrap()
                                };
                                if groups.folder(&folder).is_none() {
                                    Err("Folder not found".into())
                                } else {
                                    groups.add_to_folder(&folder, id);
                                    let name = groups.folder(&folder).unwrap().name.clone();
                                    self.app_background(call,move|store|{locked(&store)?.save_groups(&project,&groups)?;Ok((json!({"sessionId":id,"folderId":folder,"folderName":name}),Effect::Groups(project,groups)))},cx);
                                    return;
                                }
                            }
                        }
                    }
                }
            },
            "worktrees.list" | "worktrees.create" => {
                self.app_background(
                    call,
                    move |_| {
                        let root = std::path::Path::new(&project);
                        let value = if action == "worktrees.list" {
                            let snapshot = monocode_core::git::snapshot(root)?;
                            json!({"worktrees":snapshot.worktrees.iter().map(worktree_json).collect::<Vec<_>>(),"branches":snapshot.branches.iter().map(|b|json!({"name":b.name,"remote":b.remote})).collect::<Vec<_>>()})
                        } else {
                            let branch = app::text(&input, "branch", 400)?;
                            let base = app::optional_text(&input, "base", 400)?;
                            let existing = input["existing"].as_bool().unwrap_or(false);
                            if existing && base.is_some() {
                                return Err("base cannot be set for an existing branch".into());
                            }
                            worktree_json(&monocode_core::git::create_project_worktree(
                                root,
                                &branch,
                                base.as_deref(),
                                existing
                            )?)
                        };
                        Ok((value, Effect::None))
                    },
                    cx,
                );
                return;
            }
            _ => Err("Unknown Operator action".into()),
        };
        self.finish_app_call(call, result, cx);
    }
}
fn mutating(action: &str) -> bool {
    !matches!(
        action,
        "models.list"
            | "sessions.list"
            | "sessions.read"
            | "folders.list"
            | "notes.list"
            | "notes.read"
            | "worktrees.list"
    )
}
fn worktree_json(tree: &monocode_core::git::Worktree) -> Value {
    json!({"path":tree.path,"branch":tree.branch,"head":tree.head,"main":tree.main,"missing":tree.missing})
}
fn session_id(input: &Value, key: &str, store: &Database) -> Result<i64, String> {
    match input.get(key) {
        Some(Value::String(id)) => locked(store)?.resolve_session_id(id),
        Some(value) => value.as_i64().ok_or_else(|| format!("Provide {key}")),
        None => Err(format!("Provide {key}")),
    }
}
