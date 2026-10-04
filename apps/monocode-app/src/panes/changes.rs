//! The source control sidebar tab.
use gpui::{
    AnyView, App, AppContext as _, Context, Entity, IntoElement, Render, Subscription, Task, Window,
};
use monocode_engine::remote::{RemoteGlobal, connections};
use monocode_engine::{runtime::Engine, workspace::Workspace};
use monocode_layout::{
    CommitTabSource, GitFileDiffKind, WorkspaceTab, focused_file_tab, is_filesystem_tab,
};
use monocode_view_scm::ui::changes_panel::{ChangesPanelEvent, GitChangesPanel};

pub fn view(window: &mut Window, cx: &mut App) -> Option<AnyView> {
    crate::slots::cached_view("changes", window, cx, |window, cx| {
        let workspace = crate::slots::window_workspace_for(window, cx)?;
        Some(cx.new(|cx| Changes::new(workspace, window, cx)).into())
    })
}
struct Changes {
    workspace: Entity<Workspace>,
    panel: Entity<GitChangesPanel>,
    cwd: String,
    selection: Option<Selection>,
    _events: Subscription,
    _subscriptions: Vec<Subscription>,
    _remote_watch: Option<(monocode_settings::Subscription, Task<()>)>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Selection {
    path: Option<String>,
    kind: Option<monocode_view_scm::GitFileDiffKind>,
    sha: Option<String>,
}

impl Selection {
    fn from_tab(tab: Option<&WorkspaceTab>, cwd: &str) -> Self {
        let Some(tab) = tab else {
            return Self::default();
        };
        let focused = focused_file_tab(tab);
        let review = focused.filter(|file| file.review == Some(true));
        let path = review
            .filter(|file| is_filesystem_tab(file))
            .map(|file| monocode_core::paths::display_path(&file.path, Some(cwd)));
        let kind = review
            .and_then(|file| file.change_kind)
            .map(|kind| match kind {
                GitFileDiffKind::Staged => monocode_view_scm::GitFileDiffKind::Staged,
                GitFileDiffKind::Unstaged => monocode_view_scm::GitFileDiffKind::Unstaged,
            });
        let commit = focused.and_then(|file| file.commit.as_ref()).or_else(|| {
            tab.editor_panes.iter().find_map(|pane| {
                pane.files
                    .iter()
                    .find(|file| file.id == pane.active_file_id)?
                    .commit
                    .as_ref()
            })
        });
        Self {
            path,
            kind,
            sha: commit.map(|commit| commit.sha.clone()),
        }
    }
}

/// The layout's copy of the panel's diff side.
fn layout_kind(kind: monocode_view_scm::GitFileDiffKind) -> GitFileDiffKind {
    match kind {
        monocode_view_scm::GitFileDiffKind::Staged => GitFileDiffKind::Staged,
        monocode_view_scm::GitFileDiffKind::Unstaged => GitFileDiffKind::Unstaged,
    }
}

impl Changes {
    fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.cwd != self.workspace.read(cx).git_cwd(cx) {
            let (panel, events, cwd) = Self::panel(&self.workspace.clone(), window, cx);
            self.panel = panel;
            self.cwd = cwd;
            self._events = events;
            self.selection = None;
        }
        self.sync(cx);
        cx.notify();
    }

    fn sync(&mut self, cx: &mut Context<Self>) {
        let workspace = self.workspace.read(cx);
        let harness = workspace.active_session(cx).map(|session| session.harness);
        let selection = Selection::from_tab(workspace.active_tab(), &self.cwd);
        let changed = self.selection.as_ref() != Some(&selection);
        self.panel.update(cx, |panel, cx| {
            panel.set_text_harness(harness);
            if changed {
                panel.set_selection(
                    selection.path.clone(),
                    selection.kind,
                    selection.sha.clone(),
                    cx,
                );
            }
        });
        self.selection = Some(selection);
    }

    fn panel(
        workspace: &Entity<Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (Entity<GitChangesPanel>, Subscription, String) {
        let cwd = workspace.read(cx).git_cwd(cx);
        let scm = crate::adapters::scm::app_scm(cx);
        let panel = cx.new(|cx| GitChangesPanel::new(scm, cwd.clone(), true, window, cx));
        let target = workspace.downgrade();
        let events = cx.subscribe(&panel, move |_, _, event: &ChangesPanelEvent, cx| {
            let Some(workspace) = target.upgrade() else {
                return;
            };
            workspace.update(cx, |workspace, cx| match event {
                ChangesPanelEvent::OpenFile { path, kind, pin } => {
                    workspace
                        .open_working_tree_diff(path, Some(layout_kind(*kind)), *pin, cx)
                        .detach();
                }
                ChangesPanelEvent::OpenAllChanges { kind } => {
                    workspace.open_all_changes(Some(layout_kind(*kind)), cx)
                }
                ChangesPanelEvent::OpenCommit { commit, pin } => workspace.open_commit(
                    CommitTabSource::new(&commit.sha, &commit.short_sha, &commit.subject),
                    *pin,
                    cx,
                ),
            });
        });
        (panel, events, cwd)
    }
    fn new(workspace: Entity<Workspace>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (panel, events, cwd) = Self::panel(&workspace, window, cx);
        let observed = cx.observe_in(&workspace, window, |this, _, window, cx| {
            this.refresh(window, cx);
        });
        let mut subscriptions = vec![observed];
        if let Some(engine) = Engine::try_global(cx) {
            subscriptions.push(cx.observe_in(
                &engine.sessions.clone(),
                window,
                |this, _, window, cx| {
                    this.refresh(window, cx);
                },
            ));
        }
        let remote = RemoteGlobal::try_global(cx).map(|remote| remote.connections.clone());
        let remote_watch = remote.map(|remote| {
            let kv = remote.read(cx).kv().clone();
            subscriptions.push(cx.observe_in(&remote, window, |this, _, window, cx| {
                this.refresh(window, cx);
            }));
            let (send, receive) = async_channel::bounded(1);
            let lease = kv.subscribe(move |change| {
                if matches!(
                    change.key.as_str(),
                    connections::TAB_KEY | connections::WORKTREE_KEY
                ) || change.key.starts_with("monocode.remote-history.v2:")
                {
                    let _ = send.try_send(());
                }
            });
            let task = cx.spawn_in(window, async move |this, cx| {
                while receive.recv().await.is_ok() {
                    if this
                        .update_in(cx, |this, window, cx| this.refresh(window, cx))
                        .is_err()
                    {
                        break;
                    }
                }
            });
            (lease, task)
        });
        let mut this = Self {
            workspace,
            panel,
            cwd,
            selection: None,
            _events: events,
            _subscriptions: subscriptions,
            _remote_watch: remote_watch,
        };
        this.sync(cx);
        this
    }
}
impl Render for Changes {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.panel.clone()
    }
}

#[cfg(test)]
mod tests {
    use monocode_layout::{FilePaneTab, leaf, new_editor_pane};

    use super::*;

    #[test]
    fn selection_tracks_diff_kind_and_the_visible_commit() {
        let mut file = FilePaneTab::new("review", "/repo/src/app.rs", "/repo");
        file.review = Some(true);
        file.change_kind = Some(GitFileDiffKind::Staged);
        let review_pane = new_editor_pane(file);
        let mut tab = WorkspaceTab::new("tab", leaf(&review_pane.id), &review_pane.id);
        tab.editor_panes.push(review_pane);
        let selected = Selection::from_tab(Some(&tab), "/repo");
        assert_eq!(selected.path.as_deref(), Some("src/app.rs"));
        assert_eq!(
            selected.kind,
            Some(monocode_view_scm::GitFileDiffKind::Staged)
        );
        assert_eq!(selected.sha, None);

        let mut commit = FilePaneTab::new("commit", "commit:abc", "/repo");
        commit.commit = Some(CommitTabSource::new("abc", "abc", "Update app"));
        tab.editor_panes.push(new_editor_pane(commit));
        assert_eq!(
            Selection::from_tab(Some(&tab), "/repo").sha.as_deref(),
            Some("abc")
        );

        tab.focused_id = tab.editor_panes[1].id.clone();
        let selected = Selection::from_tab(Some(&tab), "/repo");
        assert_eq!(selected.path, None);
        assert_eq!(selected.kind, None);
        assert_eq!(selected.sha.as_deref(), Some("abc"));
    }
}
