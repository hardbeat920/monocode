use super::*;
use monocode_core::agent::Settings;
pub(crate) struct MonoSession {
    pub id: i64,
    pub title: String,
    pub cwd: String,
    pub busy: bool,
    pub has_messages: bool,
    pub archived: bool,
    pub updated_at: u64,
    pub needs_input: bool,
    pub branch: Option<String>,
    pub model: SharedString,
}
impl Shell {
    pub(crate) fn mono_install_app(
        &mut self,
        summary: Session,
        settings: Settings,
        draft: bool,
        cx: &mut Context<Self>,
    ) {
        let id = summary.id;
        self.draft_sessions.insert(id);
        self.session_worktrees
            .insert(id, settings.worktree_cwd.clone());
        self.add_loaded(summary, cx);
        self.sessions[&id].update(cx, |view, cx| view.install_app_launch(settings, !draft, cx));
        self.invalidate_chrome(cx);
    }
    pub(crate) fn close_mono_overview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.set_right_panel_open(false, window, cx);
    }
    /// As a restart would: the store's session list omits hidden launches.
    #[cfg(test)]
    pub(crate) fn mono_forget_session(&mut self, id: i64) {
        self.order.retain(|s| *s != id);
        self.session_summaries.remove(&id);
        self.sessions.remove(&id);
    }
    #[cfg(test)]
    pub(crate) fn mono_manager(&self) -> Entity<crate::monos::Monos> {
        self.monos.as_ref().unwrap().clone()
    }
    #[cfg(all(target_os = "macos", not(test)))]
    pub(crate) fn mono_quick_compose(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_quick_composer(window, cx);
    }
    pub(super) fn mono_active(&self, cx: &App) -> bool {
        self.monos
            .as_ref()
            .is_some_and(|m| m.read(cx).active.is_some())
    }
    pub(crate) fn mono_session_views(&self, cx: &App) -> Vec<MonoSession> {
        // A Mono's launches are usually hidden from the sidebar, so they are
        // known by summary only, outside the sidebar's order.
        let listed: HashSet<_> = self.order.iter().collect();
        let hidden = self
            .session_summaries
            .keys()
            .filter(|id| !listed.contains(id));
        self.order
            .iter()
            .chain(hidden)
            .map(|&id| {
                let metadata = self.session_metadata(id, cx);
                let live = self.sessions.get(&id).map(|v| v.read(cx));
                MonoSession {
                    id,
                    title: metadata.summary.title.clone(),
                    cwd: metadata.summary.cwd.clone(),
                    busy: metadata.busy || live.is_some_and(|v| v.mono_external_pending()),
                    has_messages: metadata.summary.has_messages,
                    archived: metadata.summary.archived,
                    updated_at: metadata.summary.updated_at,
                    needs_input: metadata.needs_approval(),
                    branch: live.and_then(|v| v.branch_name()).map(str::to_owned),
                    model: live.map(|v| v.mono_model_label()).unwrap_or_default(),
                }
            })
            .collect()
    }
    /// Load the sessions a Mono launched that the sidebar hides, so its panel
    /// can list and open them. They stay out of the sidebar.
    pub(crate) fn mono_track_sessions(&mut self, ids: Vec<i64>, cx: &mut Context<Self>) {
        let missing: Vec<_> = ids
            .into_iter()
            .filter(|id| !self.session_summaries.contains_key(id) && !self.deleting.contains(id))
            .collect();
        if missing.is_empty() {
            return;
        }
        let store = self.store.clone();
        let work = cx.background_executor().spawn(async move {
            let store = locked(&store)?;
            Ok::<_, String>(
                missing
                    .into_iter()
                    .filter_map(|id| store.mono_session(id).ok().flatten())
                    .collect::<Vec<_>>(),
            )
        });
        cx.spawn(async move |this, cx| {
            let Ok(found) = work.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                for summary in found {
                    if this.session_summaries.contains_key(&summary.id) {
                        continue;
                    }
                    this.sidebar_hidden_sessions.insert(summary.id);
                    this.session_summaries.insert(summary.id, summary);
                }
                if let Some(monos) = &this.monos {
                    monos.update(cx, |_, cx| cx.notify());
                }
            });
        })
        .detach();
    }
    pub(crate) fn mono_session_entity(&self, id: i64) -> Option<Entity<Conversation>> {
        self.sessions.get(&id).cloned()
    }
    pub(crate) fn mono_ensure_session(
        &mut self,
        id: i64,
        cx: &mut Context<Self>,
    ) -> Option<Entity<Conversation>> {
        self.ensure_session(id, cx)
    }
    pub(crate) fn mono_groups(&self, project: &str) -> monocode_core::session_groups::Groups {
        self.groups.get(project).cloned().unwrap_or_default()
    }
    pub(crate) fn mono_apply_groups(
        &mut self,
        project: String,
        groups: monocode_core::session_groups::Groups,
        cx: &mut Context<Self>,
    ) {
        self.groups.insert(project, groups);
        self.invalidate_chrome(cx);
    }
    pub(crate) fn mono_remove(
        &mut self,
        id: i64,
        delete: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if delete {
            self.delete_session(id, window, cx);
        } else if self.session_summaries.contains_key(&id)
            && !self.session_metadata(id, cx).summary.archived
        {
            self.toggle_archive(id, cx);
        }
    }
}
