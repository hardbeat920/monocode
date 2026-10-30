//! Capability-checked app actions used by Mono provider subprocesses.
use super::*;
use monocode_core::mono::{Document, Launch, bridge::Call, files, memory};
use monocode_core::{app_access as app, session_groups::Groups, time::now_ms};
use serde_json::{Value, json};
use std::path::Path;
enum Effect {
    Card(Value),
    None,
    Start(
        Session,
        Box<Settings>,
        String,
        bool,
        Option<(i64, crate::pane_layout::Edge)>,
    ),
    Groups(String, Groups),
}
fn text(v: &Value, key: &str, max: usize) -> Result<String, String> {
    v.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty() && s.len() <= max)
        .map(str::to_owned)
        .ok_or_else(|| format!("Provide {key} (up to {max} bytes)"))
}
fn id(v: &Value, key: &str) -> Result<i64, String> {
    v.get(key)
        .and_then(|v| v.as_i64().or_else(|| v.as_str()?.parse().ok()))
        .filter(|n| *n > 0)
        .ok_or_else(|| format!("Provide {key}"))
}
fn flag(v: &Value, key: &str, default: bool) -> bool {
    v.get(key).and_then(Value::as_bool).unwrap_or(default)
}
fn fingerprint(call: &Call) -> String {
    files::content_hash(&format!("{}:{}", call.action, call.input))
}
impl Monos {
    fn app_caller(&self, call: &Call) -> Result<Mono, String> {
        let mono = self
            .roster
            .of_session(call.session)
            .or_else(|| {
                self.running
                    .get(&call.session)
                    .and_then(|r| self.roster.get(&r.mono))
            })
            .cloned()
            .ok_or("This Mono no longer owns an active app capability")?;
        if call.action.starts_with("habits.")
            && call.action != "habits.list"
            && self.running.contains_key(&call.session)
        {
            return Err("A habit run cannot schedule more runs".into());
        }
        Ok(mono)
    }
    fn finish_call(&mut self, call: Call, result: Result<Value, String>, cx: &mut Context<Self>) {
        let key = (call.session, call.request_id.clone());
        self.app_pending.remove(&key);
        if self.app_results.len() > 1024
            && let Some(key) = self.app_results.keys().next().cloned()
        {
            self.app_results.remove(&key);
        }
        self.app_results
            .insert(key, (fingerprint(&call), result.clone()));
        let store = self.store.clone();
        let receipt_key = format!(
            "mono-app-receipt:{}:{}",
            call.session,
            files::content_hash(&call.request_id)
        );
        let receipt =
            serde_json::to_string(&(fingerprint(&call), result.clone())).unwrap_or_default();
        let previous = self.writes.take();
        self.writes = Some(cx.spawn(async move |_, cx| {
            if let Some(previous) = previous {
                previous.await;
            }
            let work = cx
                .background_executor()
                .spawn(async move { locked(&store)?.save_setting(&receipt_key, &receipt) });
            let reply = work
                .await
                .map(|_| ())
                .map_err(|e| format!("Could not persist app result: {e}"));
            let _ = call.reply.send(reply.and(result));
        }));
    }
    fn call_work(
        &mut self,
        call: Call,
        mono_id: String,
        work: impl FnOnce(Database) -> Result<(Value, Effect), String> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        let store = self.store.clone();
        let work = cx.background_executor().spawn(async move { work(store) });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update_in(cx, |this, window, cx| {
                let result = result.map(|(value, effect)| {
                    match effect {
                        Effect::None => (),
                        Effect::Card(value) => {
                            this.hold_card(call.session, &mono_id, value);
                            this.save(cx);
                        }
                        Effect::Groups(project, groups) => {
                            let _ = this.owner.update(cx, |owner, cx| {
                                owner.mono_apply_groups(project, groups, cx)
                            });
                        }
                        Effect::Start(summary, settings, _prompt, draft, placement) => {
                            let id = summary.id;
                            let caller = call.session;
                            let origin = this
                                .chats
                                .values()
                                .chain(this.running.values().map(|r| &r.view))
                                .find(|v| v.read(cx).summary.id == caller)
                                .map(|v| v.read(cx).mono_turn)
                                .unwrap_or(0);
                            if let Some(mono) = this.roster.get_mut(&mono_id) {
                                mono.launches.push(Launch {
                                    session_id: id,
                                    cwd: summary.cwd.clone(),
                                    title: summary.title.clone(),
                                    harness: summary.provider.clone(),
                                    model: settings.model.clone(),
                                    origin_turn: ((caller as u64) << 32) | origin,
                                    notify_on_complete: flag(
                                        &call.input,
                                        "notifyOnComplete",
                                        !draft,
                                    ),
                                    completed: false,
                                    dismissed: false,
                                });
                            }
                            let _ = this.owner.update(cx, |owner, cx| {
                                owner.mono_install_app(summary, *settings, draft, cx)
                            });
                            this.save(cx);
                            this.group_launch(&mono_id, id, cx);
                            // Like Tauri, Mono launches stay in the background
                            // even if the agent asks to reveal a tab. Only an
                            // explicit split places a new session in the workspace.
                            if placement.is_some() {
                                let _ = this.owner.update(cx, |owner, cx| {
                                    owner.app_place_launch(id, placement, true, window, cx)
                                });
                            }
                        }
                    }
                    this.reload_files(&mono_id, cx);
                    this.refresh_cards(cx);
                    this.changed(cx);
                    value
                });
                this.finish_call(call, result, cx);
            });
        })
        .detach();
    }
    pub(super) fn app_call(&mut self, call: Call, _window: &mut Window, cx: &mut Context<Self>) {
        if let Err(error) = self.app_caller(&call) {
            let _ = call.reply.send(Err(error));
            return;
        }
        if let Err(error) = mono::api::validate(&call.action, &call.input) {
            let _ = call.reply.send(Err(error));
            return;
        }
        let key = (call.session, call.request_id.clone());
        if let Some((hash, result)) = self.app_results.get(&key) {
            let _ = call.reply.send(if hash == &fingerprint(&call) {
                result.clone()
            } else {
                Err("Request ID was already used with different input".into())
            });
            return;
        }
        if !self.app_pending.insert(key.clone()) {
            let _ = call.reply.send(Err(
                "Request is still running; retry with the same request ID".into(),
            ));
            return;
        }
        let receipt_key = format!(
            "mono-app-receipt:{}:{}",
            call.session,
            files::content_hash(&call.request_id)
        );
        let store = self.store.clone();
        let lookup = cx.background_executor().spawn(async move {
            locked(&store)?
                .setting(&receipt_key)?
                .map(|raw| {
                    serde_json::from_str::<(String, Result<Value, String>)>(&raw)
                        .map_err(|error| error.to_string())
                })
                .transpose()
        });
        cx.spawn(async move |this, cx| {
            let stored = lookup.await;
            let _ = this.update_in(cx, |this, window, cx| {
                // Capabilities may have been revoked while storage was busy.
                let mono = match this.app_caller(&call) {
                    Ok(mono) => mono,
                    Err(error) => {
                        this.app_pending.remove(&key);
                        let _ = call.reply.send(Err(error));
                        return;
                    }
                };
                match stored {
                    Ok(None) => {
                        this.app_call_ready(call, mono, window, cx);
                    }
                    Ok(Some((hash, result))) => {
                        this.app_pending.remove(&key);
                        let _ = call.reply.send(if hash == fingerprint(&call) {
                            result
                        } else {
                            Err("Request ID was already used with different input".into())
                        });
                    }
                    Err(error) => {
                        this.app_pending.remove(&key);
                        // A failed read is not a missing receipt: replaying the
                        // action could repeat a mutation that already committed.
                        let _ = call
                            .reply
                            .send(Err(format!("Could not read app result: {error}")));
                    }
                }
            });
        })
        .detach();
    }
    fn app_call_ready(
        &mut self,
        call: Call,
        mono: Mono,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !call.input.is_object() {
            self.finish_call(call, Err("Input must be a JSON object".into()), cx);
            return;
        }
        let input = call.input.clone();
        let action = call.action.clone();
        let root = self.root.clone();
        let target = mono.id.clone();
        let project = if action.starts_with("sessions.")
            || action.starts_with("folders.")
            || action.starts_with("worktrees.")
        {
            match mono.project(input.get("project").and_then(Value::as_str)) {
                Ok(p) => p,
                Err(e) => {
                    self.finish_call(call, Err(e), cx);
                    return;
                }
            }
        } else {
            String::new()
        };
        let hidden = self.roster.hidden_sessions.clone();
        let source = self
            .chats
            .get(&mono.id)
            .or_else(|| self.running.get(&call.session).map(|r| &r.view))
            .cloned();
        let result = match action.as_str() {
            "models.list" => {
                let profiles = Kind::ALL
                    .into_iter()
                    .map(|kind| {
                        let profile = source
                            .as_ref()
                            .filter(|s| s.read(cx).summary.provider == kind.id())
                            .map(|s| s.read(cx).settings.account.clone())
                            .unwrap_or_else(|| {
                                crate::accounts::get(cx)
                                    .map(|center| center.read(cx).selected(kind, &project))
                                    .unwrap_or_default()
                            });
                        (kind, profile)
                    })
                    .collect();
                self.call_work(
                    call,
                    target,
                    move |_| Ok((app::models_catalog(profiles)?, Effect::None)),
                    cx,
                );
                return;
            }
            "sessions.start" => {
                let parsed = (|| {
                    let prompt = app::prompt(&input)?;
                    let provider = input["harness"]
                        .as_str()
                        .map(str::to_owned)
                        .or_else(|| source.as_ref().map(|v| v.read(cx).summary.provider.clone()))
                        .unwrap_or_else(|| "codex".into());
                    let kind = Kind::parse(&provider).ok_or("Unknown provider")?;
                    let source_provider = source
                        .as_ref()
                        .map(|v| v.read(cx).summary.provider.clone())
                        .unwrap_or_else(|| provider.clone());
                    let mut settings = source
                        .as_ref()
                        .map(|v| v.read(cx).settings.clone())
                        .unwrap_or_default();
                    if source
                        .as_ref()
                        .is_none_or(|v| v.read(cx).summary.cwd != project)
                    {
                        settings.worktree_cwd = None;
                    }
                    if source_provider != provider {
                        settings.account = crate::accounts::get(cx)
                            .map(|center| center.read(cx).selected(kind, &project))
                            .unwrap_or_default();
                    }
                    let placement = self
                        .owner
                        .update(cx, |owner, cx| {
                            owner.app_placement(&input, call.session, &project, cx)
                        })
                        .map_err(|e| e.to_string())??;
                    Ok::<_, String>((prompt, provider, kind, source_provider, settings, placement))
                })();
                let (prompt, provider, kind, source_provider, settings, placement) = match parsed {
                    Ok(value) => value,
                    Err(error) => {
                        self.finish_call(call, Err(error), cx);
                        return;
                    }
                };
                let draft = flag(&input, "draft", false);
                let request = call.request_id.clone();
                self.call_work(
                    call,
                    target,
                    move |store| {
                        let models = if input.get("model").is_some()
                            || input.get("modelSettings").is_some()
                            || input.get("effort").is_some()
                        {
                            monocode_core::catalog::models_for(kind, &settings.account)?
                        } else {
                            monocode_core::catalog::cached_models_for(kind, &settings.account)
                                .unwrap_or_default()
                        };
                        let mut settings = app::launch_settings(
                            &input,
                            &source_provider,
                            settings,
                            kind,
                            &models,
                        )?;
                        settings.revision = 0;
                        let mut summary = locked(&store)?.create(&provider, &project)?;
                        app::launch_checkout(&input, &project, summary.id, &mut settings)?;
                        summary.title = app::optional_text(&input, "title", 160)?
                            .unwrap_or_else(|| prompt.chars().take(160).collect());
                        // Worktree creation can run git; do not hold the
                        // application store lock while that command runs.
                        let db = locked(&store)?;
                        db.title(summary.id, &summary.title)?;
                        db.configure_agent(summary.id, &provider, &settings)?;
                        db.prepare_app_prompt(summary.id, &request, &prompt)?;
                        summary.has_messages = true;
                        Ok((
                            json!({"sessionId":summary.id,"accepted":true,"project":project}),
                            Effect::Start(summary, Box::new(settings), prompt, draft, placement),
                        ))
                    },
                    cx,
                );
                return;
            }
            "sessions.list" | "sessions.read" => {
                let views = self
                    .owner
                    .upgrade()
                    .map(|owner| owner.read(cx).mono_session_views(cx))
                    .unwrap_or_default();
                self.call_work(call,target,move|store|{
                    let db=locked(&store)?;let all=db.sessions()?;
                    let sessions:Vec<_>=all.iter().filter(|s|!hidden.contains(&s.id)&&s.cwd==project).collect();
                    let value=if action=="sessions.list" {
                        let drafts=db.draft_sessions()?;let metadata=db.mobile_session_metadata()?;
                        json!({"cwd":project,"sessions":sessions.iter().map(|s|json!({"id":s.id,"sessionId":s.id,"title":s.title,"harness":s.provider,"model":metadata.get(&s.id).and_then(|m|m.model.clone()),"busy":views.iter().any(|v|v.id==s.id&&v.busy),"hasDraft":drafts.contains(&s.id),"archived":s.archived})).collect::<Vec<_>>()})
                    }else{
                        let session=if let Some(id)=input["sessionId"].as_str(){db.resolve_session_id(id)?}else{id(&input,"sessionId")?};
                        if !sessions.iter().any(|s|s.id==session){return Err("Session is outside the assigned project".into());}
                        db.app_conversation(session,&input,views.iter().any(|v|v.id==session&&v.busy),db.draft_sessions()?.contains(&session))?
                    };
                    Ok((value,Effect::None))
                },cx);
                return;
            }
            "sessions.send" | "sessions.draft" | "sessions.stop" | "sessions.archive"
            | "sessions.delete" => {
                let session = match id(&input, "sessionId") {
                    Ok(id) => id,
                    Err(e) => {
                        self.finish_call(call, Err(e), cx);
                        return;
                    }
                };
                let view = self
                    .owner
                    .upgrade()
                    .and_then(|owner| {
                        owner.update(cx, |owner, cx| owner.mono_ensure_session(session, cx))
                    })
                    .filter(|v| {
                        monocode_core::files::path_key(&v.read(cx).summary.cwd)
                            == monocode_core::files::path_key(&project)
                    });
                let Some(view) = view else {
                    self.finish_call(
                        call,
                        Err("Session is outside the assigned project".into()),
                        cx,
                    );
                    return;
                };
                if action == "sessions.send" || action == "sessions.draft" {
                    if view.read(cx).busy
                        || view.read(cx).mono_external_pending()
                        || !view.read(cx).input.read(cx).content.trim().is_empty()
                    {
                        self.finish_call(
                            call,
                            Err("Session is busy or has an unsent draft".into()),
                            cx,
                        );
                        return;
                    }
                    match text(&input, "prompt", 65536) {
                        Ok(prompt) => {
                            let sent = view.update(cx, |v, cx| {
                                v.mono_external_send(prompt, action == "sessions.draft", cx)
                            });
                            if let Err(e) = sent {
                                self.finish_call(call, Err(e), cx);
                                return;
                            }
                            if flag(&input, "notifyOnComplete", false) {
                                if let Some(m) = self.roster.get_mut(&mono.id) {
                                    let origin = source.map(|v| v.read(cx).mono_turn).unwrap_or(0);
                                    if let Some(launch) =
                                        m.launches.iter_mut().find(|l| l.session_id == session)
                                    {
                                        launch.completed = false;
                                        launch.dismissed = false;
                                        launch.notify_on_complete = true;
                                        launch.origin_turn = ((call.session as u64) << 32) | origin;
                                    } else {
                                        let summary = &view.read(cx).summary;
                                        m.launches.push(Launch {
                                            session_id: session,
                                            cwd: summary.cwd.clone(),
                                            title: summary.title.clone(),
                                            harness: summary.provider.clone(),
                                            model: view.read(cx).mono_model(),
                                            origin_turn: ((call.session as u64) << 32) | origin,
                                            notify_on_complete: true,
                                            completed: false,
                                            dismissed: false,
                                        });
                                    }
                                }
                                self.save(cx);
                            }
                            Ok(json!({"accepted":true}))
                        }
                        Err(e) => Err(e),
                    }
                } else {
                    for m in &mut self.roster.monos {
                        for launch in &mut m.launches {
                            if launch.session_id == session {
                                launch.dismissed = true;
                            }
                        }
                    }
                    self.save(cx);
                    if action == "sessions.stop" {
                        view.update(cx, |v, cx| v.stop_if_busy(cx));
                    } else {
                        let delete = action == "sessions.delete";
                        let owner = self.owner.clone();
                        let handle = window.window_handle();
                        let app: &mut App = cx;
                        app.defer(move |cx| {
                            let _ = handle.update(cx, |_, window, cx| {
                                let _ = owner.update(cx, |owner, cx| {
                                    owner.mono_remove(session, delete, window, cx)
                                });
                            });
                        });
                    }
                    Ok(json!({"accepted":true}))
                }
            }
            "folders.list" => Ok(json!(
                self.owner
                    .upgrade()
                    .map(|o| o.read(cx).mono_groups(&project))
                    .unwrap_or_default()
                    .folders
            )),
            "folders.move" => {
                let session = match id(&input, "sessionId") {
                    Ok(id) => id,
                    Err(e) => {
                        self.finish_call(call, Err(e), cx);
                        return;
                    }
                };
                let allowed = self
                    .owner
                    .upgrade()
                    .and_then(|o| o.update(cx, |owner, cx| owner.mono_ensure_session(session, cx)))
                    .is_some_and(|v| v.read(cx).summary.cwd == project);
                if !allowed {
                    self.finish_call(call, Err("Session is outside this project".into()), cx);
                    return;
                }
                let mut groups = self
                    .owner
                    .upgrade()
                    .map(|o| o.read(cx).mono_groups(&project))
                    .unwrap_or_default();
                if let Some(name) = input.get("newFolderName").and_then(Value::as_str) {
                    if let Some(folder) = groups.create_folder(&[session], mono::new_id()) {
                        groups.rename_folder(&folder, name);
                    }
                } else if let Some(folder) = input.get("folderId").and_then(Value::as_str) {
                    if groups.folder(folder).is_none() {
                        self.finish_call(call, Err("Folder not found".into()), cx);
                        return;
                    }
                    groups.add_to_folder(folder, session);
                } else {
                    groups.remove_sessions(&[session]);
                }
                self.call_work(
                    call,
                    target,
                    move |store| {
                        locked(&store)?.save_groups(&project, &groups)?;
                        Ok((json!({"moved":true}), Effect::Groups(project, groups)))
                    },
                    cx,
                );
                return;
            }
            "artifacts.list" => Ok(json!(
                mono.documents
                    .iter()
                    .filter(|d| input
                        .get("kind")
                        .and_then(Value::as_str)
                        .is_none_or(|k| k == d.kind))
                    .skip(input.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize)
                    .take(
                        input
                            .get("limit")
                            .and_then(Value::as_u64)
                            .unwrap_or(50)
                            .min(200) as usize
                    )
                    .collect::<Vec<_>>()
            )),
            "artifacts.read" => input
                .get("id")
                .and_then(Value::as_str)
                .and_then(|id| mono.documents.iter().find(|d| d.id == id))
                .map(|d| json!(d))
                .ok_or("Document not found".into()),
            "chat.card" => {
                let mut card = match mono::cards::parse(&input) {
                    Ok(c) => c,
                    Err(e) => {
                        self.finish_call(call, Err(e), cx);
                        return;
                    }
                };
                card["id"] = json!(mono::new_id());
                if card["type"] == "session" {
                    let session = card["sessionId"].as_i64().unwrap();
                    let view = self
                        .owner
                        .upgrade()
                        .and_then(|o| {
                            o.update(cx, |owner, cx| owner.mono_ensure_session(session, cx))
                        })
                        .filter(|v| mono.works_on(&v.read(cx).summary.cwd));
                    let Some(view) = view else {
                        self.finish_call(
                            call,
                            Err("Session is outside assigned projects".into()),
                            cx,
                        );
                        return;
                    };
                    card["title"] = json!(view.read(cx).summary.title);
                    card["state"] = json!(if view.read(cx).busy {
                        "Working"
                    } else {
                        "Saved"
                    });
                }
                if card["type"] == "pr" {
                    let cwd =
                        mono.projects.first().cloned().unwrap_or_else(|| {
                            std::env::var("HOME").unwrap_or_else(|_| ".".into())
                        });
                    self.call_work(
                        call,
                        target,
                        move |_| {
                            let metadata = mono::cards::pull_request(
                                &cwd,
                                card["repo"].as_str(),
                                card["number"].as_u64().unwrap(),
                            )?;
                            for key in
                                ["title", "state", "url", "isDraft", "additions", "deletions"]
                            {
                                card[key] = metadata[key].clone();
                            }
                            Ok((card.clone(), Effect::Card(card)))
                        },
                        cx,
                    );
                    return;
                }
                self.hold_card(call.session, &mono.id, card.clone());
                self.save(cx);
                Ok(card)
            }
            "artifacts.write" => {
                let kind = input
                    .get("kind")
                    .and_then(Value::as_str)
                    .unwrap_or("document");
                if kind == "html" {
                    Err("HTML artifacts are no longer supported. Write kind document, and show charts or visuals with visual_show in your reply.".into())
                } else if !["document", "markdown"].contains(&kind) {
                    Err("Use kind document".into())
                } else {
                    match text(&input, "title", 512)
                        .and_then(|title| Ok((title, text(&input, "body", 512 * 1024)?)))
                    {
                        Err(e) => Err(e),
                        Ok((title, body)) => {
                            let id = input
                                .get("id")
                                .and_then(Value::as_str)
                                .map(str::to_owned)
                                .unwrap_or_else(mono::new_id);
                            let now = now_ms();
                            let document = Document {
                                id: id.clone(),
                                kind: kind.into(),
                                title,
                                summary: input
                                    .get("summary")
                                    .and_then(Value::as_str)
                                    .unwrap_or_default()
                                    .chars()
                                    .take(4000)
                                    .collect(),
                                body,
                                created_at: mono
                                    .documents
                                    .iter()
                                    .find(|d| d.id == id)
                                    .map(|d| d.created_at)
                                    .unwrap_or(now),
                                updated_at: now,
                            };
                            if let Some(m) = self.roster.get_mut(&mono.id) {
                                if let Some(old) = m.documents.iter_mut().find(|d| d.id == id) {
                                    *old = document.clone();
                                } else {
                                    m.documents.push(document.clone());
                                }
                            }
                            self.documents.remove(&id);
                            self.html_documents.remove(&id);
                            self.hold_card(call.session,&mono.id,json!({"id":mono::new_id(),"type":"document","documentId":id,"title":document.title,"summary":document.summary}));
                            self.save(cx);
                            Ok(json!(document))
                        }
                    }
                }
            }
            "soul.read" | "soul.update" | "memory.read" | "memory.search" | "memory.add"
            | "memory.replace" | "memory.remove" | "habits.list" | "habits.add"
            | "habits.update" | "habits.run" | "habits.remove" | "notes.list" | "notes.read"
            | "notes.write" | "worktrees.list" | "worktrees.create" => {
                let caller = call.session;
                let request = call.request_id.clone();
                let projects = mono.projects.clone();
                self.call_work(call,target.clone(),move|store|{
                    let value=match action.as_str(){
                        "soul.read"=>json!(files::read(&root,&target,None,"SOUL.md")?),
                        "soul.update"=>{let body=input.get("text").and_then(Value::as_str).filter(|t|t.len()<=128*1024).ok_or("Provide text")?;let expected=text(&input,"expectedHash",128)?;json!({"hash":files::save(&root,&target,None,"SOUL.md",body,Some(&expected))?})},
                        "memory.read"=>json!(memory::read(&root,&target,input.get("topic").and_then(Value::as_str))?),
                        "memory.search"=>json!(memory::search(&root,&target,&text(&input,"query",4000)?,input.get("since").and_then(Value::as_str))?),
                        "memory.add"|"memory.replace"|"memory.remove"=>{let entry=if action=="memory.remove"{None}else{Some(memory::entry(&text(&input,"fact",4000)?,&memory::date(),input.get("until").and_then(Value::as_str))?)};let find=input.get("find").and_then(Value::as_str).unwrap_or_default();let hash=memory::edit(&root,&target,input.get("topic").and_then(Value::as_str),|old|if action=="memory.add"{Ok(memory::add(old,entry.as_deref().unwrap()))}else{memory::replace(old,find,entry.as_deref(),&memory::date())})?;json!({"hash":hash})},
                        "habits.list"=>json!(habits::load(&root,&target)?),
                        "habits.add"=>{let schedule:habits::Schedule=serde_json::from_value(input.get("schedule").cloned().ok_or("Provide schedule")?).map_err(|e|e.to_string())?;let h=Habit::new(text(&input,"name",480)?,text(&input,"instructions",32000)?,schedule,now_ms())?;habits::update(&root,&target,|all|{all.push(h.clone());Ok(())})?;json!(h)},
                        "habits.update"|"habits.run"|"habits.remove"=>{let id=text(&input,"id",128)?;habits::update(&root,&target,|all|{let index=all.iter().position(|h|h.id==id).ok_or("Habit not found")?;if action=="habits.remove"{all.remove(index);return Ok(json!({"removed":true}));}let h=&mut all[index];if action=="habits.run"{h.run_requested=true;}else{if let Some(name)=input.get("name").and_then(Value::as_str){if name.trim().is_empty()||name.chars().count()>120{return Err("Invalid habit name".into());}h.name=name.trim().into();}
if let Some(instructions)=input.get("instructions").and_then(Value::as_str){if instructions.trim().is_empty()||instructions.len()>32000{return Err("Invalid habit instructions".into());}h.instructions=instructions.into();}
if let Some(schedule)=input.get("schedule"){h.schedule=serde_json::from_value(schedule.clone()).map_err(|e|e.to_string())?;h.schedule.validate()?;h.next_run_at=h.schedule.next(now_ms())?;}
if let Some(enabled)=input.get("enabled").and_then(Value::as_bool){h.enabled=enabled;if enabled{h.next_run_at=h.schedule.next(now_ms())?;}}}Ok(json!(h))})?},
                        "notes.list"|"notes.read"|"notes.write"=>app::notes_action(&*locked(&store)?,caller,&request,&action,&input,&projects)?,
                        "worktrees.list"=>json!(monocode_core::git::snapshot(Path::new(&project))?.worktrees.iter().map(|w|json!({"path":w.path,"branch":w.branch,"head":w.head,"main":w.main})).collect::<Vec<_>>()),
                        "worktrees.create"=>{let branch=text(&input,"branch",240)?;let tree=monocode_core::git::create_project_worktree(Path::new(&project),&branch,input.get("base").and_then(Value::as_str),flag(&input,"existing",false))?;json!({"path":tree.path,"branch":tree.branch})},
                        _=>unreachable!(),
                    };Ok((value,Effect::None))
                },cx);
                return;
            }
            _ => Err("Unknown action. Run app help to see supported actions".into()),
        };
        self.finish_call(call, result, cx);
    }
    pub(super) fn group_launch(&mut self, id: &str, session: i64, cx: &mut Context<Self>) {
        let Some(mono) = self
            .roster
            .get(id)
            .filter(|m| m.show_started_sessions_in_sidebar && m.use_sidebar_folders)
            .cloned()
        else {
            return;
        };
        let Some(launch) = mono.launches.iter().find(|l| l.session_id == session) else {
            return;
        };
        let project = launch.cwd.clone();
        let mut groups = self
            .owner
            .upgrade()
            .map(|o| o.read(cx).mono_groups(&project))
            .unwrap_or_default();
        let folder = groups
            .folders
            .iter()
            .find(|f| f.name == mono.label())
            .map(|f| f.id.clone())
            .or_else(|| groups.create_folder(&[session], mono::new_id()));
        if let Some(folder) = folder {
            groups.rename_folder(&folder, &mono.label());
            let color = mono::COLORS_MONO
                .iter()
                .position(|c| *c == mono.color)
                .map(|n| if n == 8 { 6 } else { n + 1 });
            groups.set_color(&folder, color);
            groups.add_to_folder(&folder, session);
        }
        let _ = self.owner.update(cx, |o, cx| {
            o.mono_apply_groups(project.clone(), groups.clone(), cx)
        });
        let store = self.store.clone();
        cx.background_executor()
            .spawn(async move {
                let _ = locked(&store).and_then(|db| db.save_groups(&project, &groups));
            })
            .detach();
    }
    pub(super) fn check_completions(&mut self, cx: &mut Context<Self>) {
        let views = self
            .owner
            .upgrade()
            .map(|o| o.read(cx).mono_session_views(cx))
            .unwrap_or_default();
        let mut ready = Vec::new();
        for mono in &self.roster.monos {
            let groups: HashSet<_> = mono
                .launches
                .iter()
                .filter(|l| l.notify_on_complete && !l.completed && !l.dismissed)
                .map(|l| l.origin_turn)
                .collect();
            for group in groups {
                let source = (group >> 32) as i64;
                if self
                    .chats
                    .values()
                    .chain(self.running.values().map(|r| &r.view))
                    .any(|v| v.read(cx).summary.id == source && v.read(cx).busy)
                {
                    continue;
                }
                let children: Vec<_> = mono
                    .launches
                    .iter()
                    .filter(|l| {
                        l.origin_turn == group
                            && l.notify_on_complete
                            && !l.completed
                            && !l.dismissed
                    })
                    .collect();
                if !children.is_empty()
                    && children.iter().all(|l| {
                        views
                            .iter()
                            .find(|v| v.id == l.session_id)
                            .is_none_or(|v| !v.busy && v.has_messages)
                    })
                    && !self.completion_work.contains(&(mono.id.clone(), group))
                {
                    ready.push((
                        mono.id.clone(),
                        group,
                        children
                            .iter()
                            .map(|l| (l.session_id, l.title.clone()))
                            .collect::<Vec<_>>(),
                    ));
                }
            }
        }
        for (id, group, children) in ready {
            self.completion_work.insert((id.clone(), group));
            let store = self.store.clone();
            let work = cx.background_executor().spawn(async move {
                children
                    .into_iter()
                    .map(|(id, title)| {
                        let rows = locked(&store)
                            .and_then(|db| db.load(id))
                            .map(|t| monocode_core::response::prose(&t.rows))
                            .unwrap_or_else(|_| "Session was removed".into());
                        format!(
                            "Session {id}: {title}\n{}",
                            monocode_core::response::limit(&rows, 12000)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n\n")
            });
            cx.spawn(async move |this,cx| {
                let report=work.await;
                let _=this.update(cx,|this,cx|{
                    this.completion_work.remove(&(id.clone(),group));
                    if let Some(mono)=this.roster.get_mut(&id){for launch in &mut mono.launches{if launch.origin_turn==group{launch.completed=true;}}}
                    let message=format!("[Delegated work completed]\nAll sessions in this group have stopped. Inspect their results and give an integrated update.\n\n{report}");
                    let source=(group>>32) as i64;
                    if let Some(run)=this.running.get_mut(&source){run.mailbox.push(message);this.save(cx);this.habit_changed(source,cx);}else{this.enqueue(&id,message,Vec::new(),true,cx);this.ensure_chat(&id,cx);}
                });
            }).detach();
        }
    }
}
