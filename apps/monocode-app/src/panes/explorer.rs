//! The explorer sidebar tab.
use gpui::{
    AnyView, App, AppContext as _, Context, Entity, IntoElement, Render, Subscription, Task, Window,
};
use monocode_engine::remote::{RemoteGlobal, connections};
use monocode_engine::workspace::Workspace;
use monocode_view_files::{FileTree, FileTreeEvent};

pub fn view(window: &mut Window, cx: &mut App) -> Option<AnyView> {
    crate::slots::cached_view("explorer", window, cx, |window, cx| {
        let workspace = crate::slots::window_workspace_for(window, cx)?;
        Some(cx.new(|cx| Explorer::new(workspace, window, cx)).into())
    })
}
struct Explorer {
    workspace: Entity<Workspace>,
    tree: Entity<FileTree>,
    _subscriptions: Vec<Subscription>,
    _remote_watch: Option<(monocode_settings::Subscription, Task<()>)>,
}
impl Explorer {
    fn sync_cwd(&mut self, cx: &mut Context<Self>) {
        let cwd = self.workspace.read(cx).git_cwd(cx);
        self.tree.update(cx, |tree, cx| tree.set_cwd(cwd, cx));
    }

    fn new(workspace: Entity<Workspace>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let cwd = workspace.read(cx).git_cwd(cx);
        let data = crate::adapters::files::app_files(cx);
        let tree = cx.new(|cx| FileTree::new(data, cwd, window, cx));
        tree.update(cx, |tree, cx| tree.set_open_terminal_enabled(true, cx));
        let target = workspace.downgrade();
        let events = cx.subscribe(&tree, move |_, _, event: &FileTreeEvent, cx| {
            let Some(workspace) = target.upgrade() else {
                return;
            };
            workspace.update(cx, |workspace, cx| match event {
                FileTreeEvent::OpenFile { path, options } => workspace
                    .open_file(
                        path,
                        None,
                        monocode_engine::workspace::workspace::FileOpenOptions {
                            pin: options.pin,
                            exact: options.exact,
                        },
                        cx,
                    )
                    .detach(),
                FileTreeEvent::OpenTerminal { cwd } => {
                    workspace.open_terminal(cwd, false, None, cx);
                }
                FileTreeEvent::FileMoved { from, to } => workspace.file_moved(from, to, cx),
                FileTreeEvent::FileDeleted { path } => workspace.file_deleted(path, cx),
                FileTreeEvent::Search => monocode_app::bridge::shell::ShellRequests::send(
                    monocode_app::bridge::shell::ShellRequest::OpenPage(
                        monocode_app::bridge::shell::ShellPage::Search,
                    ),
                    cx,
                ),
            });
        });
        let observed = cx.observe(&workspace, |this, _, cx| this.sync_cwd(cx));
        let mut subscriptions = vec![events, observed];
        let remote = RemoteGlobal::try_global(cx).map(|remote| remote.connections.clone());
        let remote_watch = remote.map(|remote| {
            let kv = remote.read(cx).kv().clone();
            subscriptions.push(cx.observe(&remote, |this, _, cx| this.sync_cwd(cx)));
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
            let task = cx.spawn(async move |this, cx| {
                while receive.recv().await.is_ok() {
                    if this.update(cx, |this, cx| this.sync_cwd(cx)).is_err() {
                        break;
                    }
                }
            });
            (lease, task)
        });
        Self {
            workspace,
            tree,
            _subscriptions: subscriptions,
            _remote_watch: remote_watch,
        }
    }
}
impl Render for Explorer {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.tree.clone()
    }
}
