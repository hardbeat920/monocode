//! Project indexes are fetched on workers; opening a row materializes the chat.
use super::*;
use serde_json::json;

impl Shell {
    pub(super) fn refresh_remote_projects(&mut self, cx: &mut Context<Self>) {
        if self.remote_refreshing || !self.workspace.project.starts_with("remote://") {
            return;
        }
        self.remote_refreshing = true;
        let project_key = self.workspace.project.clone();
        let store = self.store.clone();
        let client = self.remote.clone();
        let work = cx.background_executor().spawn(async move {
            let project = locked(&store)?
                .remote_projects()?
                .into_values()
                .find(|p| p.key == project_key)
                .ok_or(
                    "Remote project identity is missing. Open this folder on its machine again.",
                )?;
            let machine = client.machine_for_environment(&project.environment_id)?;
            let response = client.request(
                &machine.id,
                "sessions.list",
                json!({"projectId":project.project_id}),
            )?;
            let values = response.as_array().ok_or("Invalid remote session list")?;
            let store = locked(&store)?;
            let mut summaries = Vec::new();
            let host_ids: HashSet<String> = values
                .iter()
                .filter_map(|value| value["id"].as_str().map(str::to_owned))
                .collect();
            store.prune_remote_sessions(&project, &host_ids)?;
            for value in values {
                let summary = store.register_remote_summary(&project, value)?;
                let state = store
                    .remote_session(summary.id)?
                    .ok_or("Remote session identity is missing")?;
                summaries.push((
                    summary,
                    state.running,
                    state.needs_input,
                    value["draft"] == true,
                ));
            }
            let retained: HashSet<i64> = store
                .sessions()?
                .iter()
                .filter(|s| s.cwd == project.key)
                .map(|s| s.id)
                .collect();
            Ok::<_, String>((project.key, summaries, retained))
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.remote_refreshing = false;
                match result {
                    Ok((project, summaries, retained)) => {
                        let removed: Vec<_> = this
                            .order
                            .iter()
                            .copied()
                            .filter(|id| {
                                this.session_summaries
                                    .get(id)
                                    .is_some_and(|s| s.cwd == project && !retained.contains(id))
                            })
                            .collect();
                        for id in removed {
                            this.remote_marks.remove(&id);
                            this.order.retain(|existing| *existing != id);
                            if let Some(view) = this.sessions.get(&id) {
                                view.update(cx, |view, cx| {
                                    view.remote_connection_error(
                                        "This conversation was deleted on the host.",
                                        cx,
                                    )
                                });
                            }
                            // Open tabs retain an explanatory placeholder. A stale
                            // tab must not be rebound to a new host conversation.
                            if this.workspace.tab_of(id).is_none() {
                                this.session_summaries.remove(&id);
                                this.sessions.remove(&id);
                            }
                        }
                        for (summary, running, waiting, draft) in summaries {
                            let id = summary.id;
                            this.remote_marks.insert(id, (running, waiting));
                            if draft {
                                this.draft_sessions.insert(id);
                            } else {
                                this.draft_sessions.remove(&id);
                            }
                            if !this.session_summaries.contains_key(&id) {
                                this.order.push(id);
                            }
                            if let Some(view) = this.sessions.get(&id) {
                                view.update(cx, |view, cx| {
                                    if !view.busy {
                                        view.summary = summary.clone();
                                        cx.notify();
                                    } else if view.summary.title != summary.title {
                                        // A running turn owns the rest; a host title is metadata.
                                        view.summary.title = summary.title.clone();
                                        cx.emit(crate::conversation::SessionChanged);
                                        cx.notify();
                                    }
                                });
                            }
                            this.session_summaries.insert(id, summary);
                        }
                        if this.workspace.project == project {
                            this.refresh_sidebar_structure(cx);
                            this.invalidate_chrome(cx);
                        }
                    }
                    Err(error) => {
                        // A transient disconnect retains the last host index.
                        // The active conversation exposes the error and Retry.
                        if let Some(id) = this.workspace.active
                            && let Some(view) =
                                this.sessions.get(&this.workspace.session_for_pane(id))
                        {
                            view.update(cx, |view, cx| view.remote_connection_error(&error, cx));
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
}

#[cfg(test)]
#[path = "remote_title_tests.rs"]
mod remote_title_tests;
