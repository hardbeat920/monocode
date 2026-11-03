//! Keep the history index independent of the much larger interactive chat state.
use super::*;

pub(super) struct Metadata<'a> {
    pub summary: &'a Session,
    pub busy: bool,
    live: Option<&'a Conversation>,
    draft: bool,
    remote_waiting: bool,
}
impl Metadata<'_> {
    pub(super) fn display_title(&self) -> String {
        monocode_core::session_title::display_title(&self.summary.title, &self.summary.provider)
            .replace('\n', " ")
    }
    pub(super) fn needs_approval(&self) -> bool {
        self.live.is_some_and(Conversation::needs_approval) || self.remote_waiting
    }
    pub(super) fn responding(&self) -> bool {
        self.live.is_some_and(Conversation::responding)
    }
    pub(super) fn has_saved_draft(&self) -> bool {
        self.live.map_or(self.draft, Conversation::has_saved_draft)
    }
}

impl Shell {
    pub(super) fn session_metadata<'a>(&'a self, id: i64, cx: &'a App) -> Metadata<'a> {
        let live = self.sessions.get(&id).map(|view| view.read(cx));
        Metadata {
            summary: live.map_or_else(|| &self.session_summaries[&id], |v| &v.summary),
            busy: live.is_some_and(|v| v.busy)
                || self.remote_marks.get(&id).is_some_and(|state| state.0),
            live,
            draft: self.draft_sessions.contains(&id),
            remote_waiting: self.remote_marks.get(&id).is_some_and(|state| state.1),
        }
    }
    /// Opening and explicit previews materialize chats; enumerating or scrolling
    /// history only reads summaries. Restored background tabs stay metadata-only.
    pub(super) fn ensure_session(
        &mut self,
        id: i64,
        cx: &mut Context<Self>,
    ) -> Option<Entity<Conversation>> {
        if let Some(view) = self.sessions.get(&id) {
            return Some(view.clone());
        }
        let summary = self.session_summaries.get(&id)?.clone();
        self.add_loaded(summary, cx);
        self.sessions.get(&id).cloned()
    }
    pub(super) fn trim_session_histories(&mut self, shown: &HashSet<i64>, cx: &mut Context<Self>) {
        const INACTIVE_COUNT: usize = 8;
        const INACTIVE_BYTES: usize = 32 * 1024 * 1024;
        let preview = self.session_preview.as_ref().map(|p| p.id);
        let mut inactive: Vec<_> = self
            .sessions
            .values()
            .filter_map(|view| {
                let chat = view.read(cx);
                let id = chat.summary.id;
                (!shown.contains(&id) && preview != Some(id) && !chat.busy && chat.history_loaded())
                    .then(|| {
                        (
                            id,
                            self.history_recency.get(&id).copied().unwrap_or(0),
                            chat.history_weight(cx),
                        )
                    })
            })
            .collect();
        inactive.sort_unstable_by_key(|&(id, recency, _)| (recency, id));
        let mut count = inactive.len();
        let mut bytes = inactive.iter().map(|(_, _, weight)| *weight).sum::<usize>();
        for (id, _, weight) in inactive {
            if count <= INACTIVE_COUNT && bytes <= INACTIVE_BYTES {
                break;
            }
            let view = self.sessions.get(&id).cloned();
            if view.is_some_and(|view| view.update(cx, |chat, cx| chat.unload_history(cx))) {
                count -= 1;
                bytes = bytes.saturating_sub(weight);
            }
        }
    }
}

/// Preserve remembered source controls when they refer to the selected model.
/// A deliberate native model override starts with fresh model-specific controls.
pub(super) fn new_session_settings(
    source: Option<monocode_core::agent::Settings>,
    model: Option<String>,
) -> monocode_core::agent::Settings {
    let mut settings = source.unwrap_or_default();
    if let Some(model) = model {
        if settings
            .model
            .as_ref()
            .is_some_and(|source| source != &model)
        {
            settings.effort = None;
            settings.fast = false;
            settings.thinking = None;
            settings.service_tier = None;
            settings.model_settings.clear();
        }
        settings.model = Some(model);
    }
    settings
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;
    use monocode_core::{agent::Settings, store::Store};

    #[gpui::test]
    fn whole_project_sidebar_keeps_seven_today_sessions_across_worktrees_lazy(
        cx: &mut TestAppContext,
    ) {
        let store = Store::open_monocode(std::path::Path::new(":memory:")).unwrap();
        let mut summaries = Vec::new();
        let mut main = HashSet::new();
        let mut feature = HashSet::new();
        for i in 0..7 {
            let mut summary = store.create("codex", "/work/project").unwrap();
            summary.has_messages = true;
            summary.updated_at = monocode_core::time::now_ms();
            if i < 3 {
                main.insert(summary.id);
            } else {
                feature.insert(summary.id);
                store
                    .save_agent_settings(
                        summary.id,
                        &Settings {
                            worktree_cwd: Some("/work/feature".into()),
                            ..Default::default()
                        },
                    )
                    .unwrap();
            }
            summaries.push(summary);
        }
        let archived = store.create("codex", "/work/project").unwrap();
        let mut archived = archived;
        archived.has_messages = true;
        archived.archived = true;
        summaries.push(archived);
        let mut other = store
            .create("codex", "/work/unlisted-other-project")
            .unwrap();
        other.has_messages = true;
        summaries.push(other);
        let database = Arc::new(Mutex::new(store));
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| Shell::new(database, summaries, "/work/project".into(), None, cx))
            })
            .unwrap()
        });
        window
            .update(cx, |shell, _, cx| {
                let entities = shell.sessions.len();
                assert_eq!(shell.sidebar_session_ids(cx).len(), 7);
                shell.worktree_focus.insert(
                    "/work/project".into(),
                    crate::worktree_switcher::WorktreeFocus {
                        path: "/work/feature".into(),
                        branch: Some("feature".into()),
                    },
                );
                assert_eq!(
                    shell
                        .sidebar_session_ids(cx)
                        .into_iter()
                        .collect::<HashSet<_>>(),
                    feature
                );
                shell.worktree_focus.insert(
                    "/work/project".into(),
                    crate::worktree_switcher::WorktreeFocus {
                        path: "/work/project".into(),
                        branch: None,
                    },
                );
                assert_eq!(
                    shell
                        .sidebar_session_ids(cx)
                        .into_iter()
                        .collect::<HashSet<_>>(),
                    main
                );
                shell.worktree_focus.clear();
                assert_eq!(shell.sidebar_session_ids(cx).len(), 7);
                assert_eq!(
                    shell.sessions.len(),
                    entities,
                    "counting and narrowing history stays metadata-only"
                );
                assert!(
                    !shell
                        .workspace
                        .projects
                        .contains(&"/work/unlisted-other-project".into()),
                    "a session cwd must not invent a project rail entry"
                );
            })
            .unwrap();
    }

    #[test]
    fn remembered_controls_survive_the_mapped_native_model_default() {
        let remembered = Settings {
            model: Some("model-a".into()),
            effort: Some("high".into()),
            model_settings: [("reasoningEffort".into(), "high".into())].into(),
            ..Default::default()
        };
        let selected = new_session_settings(Some(remembered.clone()), Some("model-a".into()));
        assert_eq!(selected.effort.as_deref(), Some("high"));
        assert_eq!(
            selected
                .model_settings
                .get("reasoningEffort")
                .map(String::as_str),
            Some("high")
        );
        let changed = new_session_settings(Some(remembered), Some("model-b".into()));
        assert_eq!(changed.model.as_deref(), Some("model-b"));
        assert!(changed.effort.is_none());
        assert!(changed.model_settings.is_empty());
    }

    #[gpui::test]
    fn restoring_large_history_materializes_only_visible_chats(cx: &mut TestAppContext) {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let mut summaries = Vec::new();
        for i in 0..1200 {
            let cwd = if i < 1000 {
                "/work/history"
            } else {
                "/work/other"
            };
            let mut session = store.create("codex", cwd).unwrap();
            session.title = format!("Saved conversation {i}");
            session.has_messages = true;
            summaries.push(session);
        }
        let first = summaries[0].id;
        let oldest = summaries[999].id;
        let mut workspace = Workspace::default();
        workspace.open(oldest);
        workspace.open(first);
        let saved = serde_json::to_string(&workspace).unwrap();
        let database = Arc::new(Mutex::new(store));
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| {
                    Shell::new(database, summaries, "/work/history".into(), Some(saved), cx)
                })
            })
            .unwrap()
        });
        window
            .update(cx, |shell, window, cx| {
                assert_eq!(shell.session_summaries.len(), 1200);
                assert_eq!(shell.sessions.len(), 1);
                assert_eq!(shell.session_marks.len(), 1);
                assert_eq!(shell.tab_marks.len(), 1);
                assert_eq!(shell.sidebar_session_ids(cx).len(), 1000);
                assert_eq!(shell.visible_tabs(cx).len(), 2);
                assert_eq!(
                    shell.tab_copy(oldest, cx).headline,
                    "Saved conversation 999"
                );
                shell.query = "saved conversation 999".into();
                assert_eq!(shell.sidebar_session_ids(cx), [oldest]);
                assert_eq!(
                    shell.sessions.len(),
                    1,
                    "filtering old history must remain metadata-only"
                );
                shell.query.clear();
                shell.run_session_menu(oldest, "pin", window, cx);
                assert!(shell.session_metadata(oldest, cx).summary.pinned);
                assert_eq!(
                    shell.sessions.len(),
                    1,
                    "metadata actions must not construct chats"
                );
                shell.open(oldest, window, cx);
                assert_eq!(shell.workspace.active, Some(oldest));
                assert_eq!(shell.sessions.len(), 2);
            })
            .unwrap();
    }
    #[gpui::test]
    fn inactive_history_cache_has_a_count_bound_and_reopens_evicted_history(
        cx: &mut TestAppContext,
    ) {
        let (window, _, active) = super::super::tests::window(cx);
        let ids = window
            .update(cx, |shell, _, cx| {
                let mut summaries = Vec::new();
                {
                    let mut store = locked(&shell.store).unwrap();
                    for i in 0..12 {
                        let summary = store.create("codex", "/work/cache").unwrap();
                        store
                            .write_rows(
                                summary.id,
                                0,
                                &[monocode_core::transcript::Row {
                                    role: "assistant".into(),
                                    text: format!("Saved answer {i}"),
                                    first: true,
                                    sent_at: None,
                                }],
                            )
                            .unwrap();
                        summaries.push(summary);
                    }
                }
                let ids = summaries.iter().map(|s| s.id).collect::<Vec<_>>();
                for summary in summaries {
                    let id = summary.id;
                    shell.add(summary, cx);
                    shell.history_clock += 1;
                    shell.history_recency.insert(id, shell.history_clock);
                    shell.sessions[&id].update(cx, |view, cx| view.load(cx));
                }
                ids
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, window, cx| {
                shell.update_chat_visibility(cx);
                assert_eq!(shell.workspace.active, Some(active));
                let retained = ids
                    .iter()
                    .filter(|&&id| shell.sessions[&id].read(cx).history_loaded())
                    .count();
                assert!(
                    retained <= 8,
                    "inactive cache must stay bounded: {retained}"
                );
                assert!(!shell.sessions[&ids[0]].read(cx).history_loaded());
                shell.open(ids[0], window, cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, _, cx| {
                let chat = shell.sessions[&ids[0]].read(cx);
                assert!(chat.history_loaded());
                assert_eq!(
                    chat.transcript.read(cx).source_rows()[0].text,
                    "Saved answer 0"
                );
            })
            .unwrap();
    }
}
