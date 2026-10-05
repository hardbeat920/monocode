//! The sidebar title. Port of src/features/source-control/ui/
//! SidebarWorktreeSwitcher.tsx: picking a working copy asks the workspace to
//! switch to it, which narrows the sidebar and the tabs to that worktree and
//! names it here. The project folder is the default, unfocused entry.

use std::collections::HashMap;

use gpui::{
    App, Context, Entity, InteractiveElement as _, IntoElement, MouseButton, MouseDownEvent,
    ParentElement as _, Render, StatefulInteractiveElement as _, Styled as _, Subscription,
    WeakEntity, Window, div, prelude::FluentBuilder as _,
};
use monocode_engine::projects::backend::Worktree;
use monocode_engine::projects::{GitStatus, GitWatch, ProjectsGlobal, WatchKind};
use monocode_engine::workspace::{Workspace, WorktreeFocus, WorktreeTabStats};
use monocode_layout::paths::pretty_cwd;
use monocode_ui::widgets::{popover_frame, tooltip};
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};
use monocode_view_scm::ui::common::{
    BoundsCell, PopoverPlacement, anchored_popover, contains, spin_icon, track_bounds,
};

use super::super::Shell;

/// What one render reads from the workspace and the worktree list.
struct SwitcherData {
    focus: Option<WorktreeFocus>,
    pending: bool,
    switch_error: Option<String>,
    stats: HashMap<String, WorktreeTabStats>,
    /// The project folder's entry.
    main: Option<Worktree>,
    /// The linked worktrees that still exist.
    worktrees: Vec<Worktree>,
    loaded: bool,
    list_error: Option<String>,
}

pub struct WorktreeSwitcher {
    shell: WeakEntity<Shell>,
    cwd: String,
    status: Option<Entity<GitStatus>>,
    _watch: Option<GitWatch>,
    open: bool,
    /// The switch error that last opened the popover.
    shown_error: Option<String>,
    /// The deleted focus a fallback was already requested for.
    fallback_for: Option<String>,
    trigger_bounds: BoundsCell,
    _subscriptions: Vec<Subscription>,
}

impl WorktreeSwitcher {
    pub fn new(shell: WeakEntity<Shell>) -> Self {
        Self {
            shell,
            cwd: String::new(),
            status: None,
            _watch: None,
            open: false,
            shown_error: None,
            fallback_for: None,
            trigger_bounds: BoundsCell::default(),
            _subscriptions: Vec::new(),
        }
    }

    fn workspace(&self, cx: &App) -> Option<Entity<Workspace>> {
        self.shell.upgrade()?.read(cx).workspace.clone()
    }

    /// `useProjectWorktrees(cwd)`: follow the sidebar's project.
    fn sync(&mut self, cx: &mut Context<Self>) {
        let cwd = self
            .shell
            .upgrade()
            .map(|shell| shell.read(cx).sidebar_cwd(cx))
            .unwrap_or_default();
        if cwd == self.cwd {
            return;
        }
        self.cwd = cwd.clone();
        self.open = false;
        self._watch = None;
        self.status = None;
        self._subscriptions.clear();
        if cwd.is_empty() || cwd == "~" || ProjectsGlobal::try_global(cx).is_none() {
            return;
        }
        let status = ProjectsGlobal::git_status(&cwd, cx);
        self._watch = Some(status.update(cx, |status, cx| status.watch(WatchKind::Worktrees, cx)));
        self._subscriptions
            .push(cx.observe(&status, |_, _, cx| cx.notify()));
        if let Some(workspace) = self.workspace(cx) {
            self._subscriptions
                .push(cx.observe(&workspace, |_, _, cx| cx.notify()));
        }
        self.status = Some(status);
    }

    fn data(&self, cx: &App) -> SwitcherData {
        let (focus, pending, switch_error, stats) = match self.workspace(cx) {
            Some(workspace) => {
                let workspace = workspace.read(cx);
                (
                    workspace.worktree_focus(&self.cwd).cloned(),
                    workspace.navigation_pending(&self.cwd),
                    workspace.navigation_error(&self.cwd).map(str::to_string),
                    workspace.worktree_tab_stats(cx),
                )
            }
            None => (None, false, None, HashMap::new()),
        };
        let snapshot = self
            .status
            .as_ref()
            .map(|status| status.read(cx).worktrees().clone())
            .unwrap_or_default();
        let list = snapshot.data.as_ref().map(|data| data.worktrees.as_slice());
        // Missing worktrees cannot be opened, so they are left out.
        let worktrees = list
            .unwrap_or_default()
            .iter()
            .filter(|tree| !tree.is_main && !tree.missing)
            .cloned()
            .collect();
        SwitcherData {
            focus,
            pending,
            switch_error,
            stats,
            main: list
                .unwrap_or_default()
                .iter()
                .find(|tree| tree.is_main)
                .cloned(),
            worktrees,
            loaded: snapshot.data.is_some(),
            list_error: snapshot.error,
        }
    }

    /// `onSelect`: ask the workspace to switch. The focus changes only once
    /// the switch lands.
    fn select(&mut self, focus: Option<WorktreeFocus>, cx: &mut Context<Self>) {
        self.open = false;
        let cwd = self.cwd.clone();
        if let Some(workspace) = self.workspace(cx) {
            workspace.update(cx, |workspace, cx| {
                workspace.select_workspace(&cwd, focus, cx)
            });
        }
        cx.notify();
    }

    fn toggle(&mut self, cx: &mut Context<Self>) {
        if !self.open
            && let Some(status) = &self.status
        {
            drop(status.update(cx, |status, cx| status.refresh_worktrees(cx)));
        }
        self.open = !self.open;
        cx.notify();
    }

    /// The effects of the TypeScript view: a switch failure reopens the
    /// popover, and a deleted worktree cannot stay focused, or new sessions
    /// would start there. The fallback is requested once and not retried
    /// while a switch is pending or failed.
    fn react(&mut self, data: &SwitcherData, cx: &mut Context<Self>) {
        if data.switch_error != self.shown_error {
            self.shown_error = data.switch_error.clone();
            if data.switch_error.is_some() {
                self.open = true;
            }
        }
        let Some(focus) = &data.focus else {
            self.fallback_for = None;
            return;
        };
        let deleted = data.loaded
            && !data
                .worktrees
                .iter()
                .any(|tree| same_path(&tree.path, &focus.path));
        if deleted
            && !data.pending
            && data.switch_error.is_none()
            && self.fallback_for.as_deref() != Some(focus.path.as_str())
        {
            self.fallback_for = Some(focus.path.clone());
            let this = cx.weak_entity();
            cx.defer(move |cx| {
                this.update(cx, |this, cx| this.select(None, cx)).ok();
            });
        }
    }

    fn title(data: &SwitcherData) -> String {
        let Some(focus) = &data.focus else {
            return "Workspace".into();
        };
        data.worktrees
            .iter()
            .find(|tree| same_path(&tree.path, &focus.path))
            .and_then(|tree| tree.branch.clone())
            .or_else(|| focus.branch.clone())
            .unwrap_or_else(|| "Detached worktree".into())
    }
}

fn same_path(a: &str, b: &str) -> bool {
    monocode_core::paths::path_key(a) == monocode_core::paths::path_key(b)
}

/// Tabs a worktree keeps open while another one is shown.
fn open_tabs(stats: Option<&WorktreeTabStats>, theme: &Theme) -> Option<gpui::AnyElement> {
    let stats = stats.filter(|stats| stats.tabs > 0)?;
    let label = format!(
        "{} open tab{}{}",
        stats.tabs,
        if stats.tabs == 1 { "" } else { "s" },
        if stats.busy { ", working" } else { "" }
    );
    Some(
        div()
            .id(gpui::SharedString::from(format!("worktree-tabs-{label}")))
            .flex()
            .flex_none()
            .items_center()
            .gap(u(4.))
            .text_px(11.)
            .text_color(theme.content(0.40))
            .tooltip(tooltip(label))
            .when(stats.busy, |el| {
                el.child(div().size(u(6.)).rounded_full().bg(theme.colors.accent))
            })
            .child(stats.tabs.to_string())
            .into_any_element(),
    )
}

impl Render for WorktreeSwitcher {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync(cx);
        let data = self.data(cx);
        self.react(&data, cx);
        let theme = Theme::of(cx).clone();
        let c = theme.colors;
        let title = Self::title(&data);
        if data.loaded
            && data.worktrees.is_empty()
            && data.focus.is_none()
            && data.switch_error.is_none()
            && !data.pending
        {
            return div()
                .min_w_0()
                .truncate()
                .text_px(theme.text.ui)
                .medium()
                .leading(theme.leading.tight)
                .child(title)
                .into_any_element();
        }

        let hint = match &data.focus {
            Some(focus) => format!(
                "{}\n{}",
                focus.branch.as_deref().unwrap_or("detached"),
                pretty_cwd(&focus.path)
            ),
            None => data
                .main
                .as_ref()
                .and_then(|main| main.branch.clone())
                .unwrap_or_else(|| "Project folder".into()),
        };
        let fill = theme.content(0.08);
        let trigger = div()
            .id("worktree-switcher")
            .debug_selector(|| "worktree-switcher".into())
            .relative()
            .flex()
            .min_w_0()
            .max_w_full()
            .h(u(26.))
            .ml(u(-6.))
            .px(u(6.))
            .items_center()
            .gap(u(8.))
            .rounded(u(theme.radius.md))
            .text_px(theme.text.ui)
            .medium()
            .leading(theme.leading.tight)
            .hover(move |style| style.bg(fill))
            .when(self.open, |el| el.bg(fill))
            .tooltip(tooltip(hint))
            .child(track_bounds(&self.trigger_bounds))
            .child(div().min_w_0().truncate().child(title))
            .child(if data.pending {
                spin_icon("worktree-switching", 14., theme.content(0.45)).into_any_element()
            } else {
                icon(IconName::ChevronsUpDown)
                    .size(u(14.))
                    .flex_none()
                    .text_color(theme.content(0.45))
                    .into_any_element()
            })
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|this, _, _, cx| this.toggle(cx)));

        let mut root = div().relative().flex().min_w_0().child(trigger);
        if !self.open {
            return root.into_any_element();
        }

        let row = |id: String,
                   selected: bool,
                   glyph: IconName,
                   label: String,
                   detail: String,
                   stats_path: &str,
                   focus: Option<WorktreeFocus>,
                   cx: &mut Context<Self>| {
            let hover = theme.content(0.05);
            div()
                .id(gpui::SharedString::from(id))
                .flex()
                .w_full()
                .items_center()
                .gap(u(8.))
                .rounded(u(theme.radius.md))
                .px(u(8.))
                .py(u(6.))
                .hover(move |style| style.bg(hover))
                .child(
                    icon(glyph)
                        .size(u(14.))
                        .flex_none()
                        .text_color(theme.content(0.50)),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w_0()
                        .child(div().truncate().text_px(12.).child(label))
                        .child(
                            div()
                                .truncate()
                                .text_px(10.)
                                .text_color(theme.content(0.40))
                                .child(detail),
                        ),
                )
                .children(open_tabs(
                    data.stats.get(&monocode_core::paths::path_key(stats_path)),
                    &theme,
                ))
                .when(selected, |el| {
                    el.child(icon(IconName::Check).size(u(14.)).flex_none())
                })
                .on_click(cx.listener(move |this, _, _, cx| this.select(focus.clone(), cx)))
        };

        let mut list = div()
            .id("worktree-switcher-list")
            .flex()
            .flex_col()
            .max_h(u(360.))
            .overflow_y_scroll()
            .p(u(4.));
        let main_path = data
            .main
            .as_ref()
            .map(|main| main.path.clone())
            .unwrap_or_else(|| self.cwd.clone());
        list = list.child(row(
            "worktree-default".into(),
            data.focus.is_none(),
            IconName::GitBranch,
            data.main
                .as_ref()
                .and_then(|main| main.branch.clone())
                .unwrap_or_else(|| "Project folder".into()),
            "Project folder · all sessions".into(),
            &main_path,
            None,
            cx,
        ));
        if !data.loaded && data.list_error.is_none() {
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .gap(u(8.))
                    .p(u(8.))
                    .text_px(12.)
                    .text_color(theme.content(0.50))
                    .child(spin_icon(
                        "worktree-switcher-loading",
                        14.,
                        theme.content(0.50),
                    ))
                    .child("Loading working copies…"),
            );
        }
        for tree in &data.worktrees {
            let selected = data
                .focus
                .as_ref()
                .is_some_and(|focus| same_path(&focus.path, &tree.path));
            let label = tree.branch.clone().unwrap_or_else(|| {
                format!("Detached {}", tree.head.chars().take(7).collect::<String>())
            });
            list = list.child(row(
                format!("worktree-{}", tree.path),
                selected,
                IconName::FolderTree,
                label,
                pretty_cwd(&tree.path),
                &tree.path,
                Some(WorktreeFocus {
                    path: tree.path.clone(),
                    branch: tree.branch.clone(),
                }),
                cx,
            ));
        }
        if let Some(error) = data.switch_error.clone().or(data.list_error.clone()) {
            list = list.child(
                div()
                    .px(u(8.))
                    .py(u(8.))
                    .text_px(11.)
                    .text_color(c.danger)
                    .child(error),
            );
        }
        let frame = div()
            .id("worktree-switcher-popover")
            .occlude()
            .on_mouse_down_out(cx.listener(|this, event: &MouseDownEvent, _, cx| {
                if !contains(&this.trigger_bounds, event.position) {
                    this.open = false;
                    cx.notify();
                }
            }))
            .child(
                popover_frame("worktree-switcher-frame")
                    .width(280.)
                    .child(list),
            );
        root = root.child(anchored_popover(
            PopoverPlacement::BottomStart,
            6.,
            theme.layer.popover,
            frame,
            window,
        ));
        root.into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::ShellOptions;
    use gpui::{AppContext as _, TestAppContext};
    use monocode_engine::history::History;
    use monocode_engine::projects::backend::Worktrees;
    use monocode_engine::projects::{ProjectsConfig, testing::FakeBackend};
    use monocode_engine::runtime::testing::init_test_engine;
    use monocode_engine::workspace::WorkspaceConfig;
    use monocode_settings::Kv;
    use std::sync::Arc;

    struct Root(Entity<Shell>);

    impl Render for Root {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }

    fn tree(path: &str, branch: &str, is_main: bool) -> Worktree {
        Worktree {
            is_main,
            ..Worktree::new(path, Some(branch))
        }
    }

    // SidebarWorktreeSwitcher.test.ts, without the delegate that moves a
    // blank session: the switch fails the way a vanished worktree does.
    #[gpui::test]
    fn a_pick_requests_a_switch_without_publishing_it_and_a_failure_reopens_the_popover(
        cx: &mut TestAppContext,
    ) {
        init_test_engine(cx);
        let backend = FakeBackend::new();
        backend.set_worktrees(
            "/picker",
            Ok(Worktrees {
                worktrees: vec![
                    tree("/picker", "main", true),
                    tree("/picker-a", "feature-a", false),
                    tree("/picker-b", "feature-b", false),
                ],
                default_root: "/picker-trees".into(),
            }),
        );
        cx.update(|cx| {
            gpui_component::init(cx);
            monocode_ui::init(monocode_ui::AppearanceSettings::default(), cx);
            ProjectsGlobal::init(
                ProjectsConfig {
                    kv: Kv::in_memory(),
                    backend,
                    clock: Arc::new(|| 1_000_000),
                },
                cx,
            );
        });
        // The root holds the shell without drawing it, so the panes' own
        // globals are not needed.
        let (root, cx) = cx.add_window_view(|window, cx| {
            let shell = cx.new(|cx| {
                let mut shell = Shell::new(ShellOptions::full(), window, cx);
                let history = cx.new(|cx| History::new(Kv::in_memory(), cx));
                shell.attach(WorkspaceConfig::fresh(Some("/picker")), history, window, cx);
                shell
            });
            Root(shell)
        });
        let shell = root.read_with(cx, |root, _| root.0.clone());
        let switcher = cx.new(|_| WorktreeSwitcher::new(shell.downgrade()));
        cx.run_until_parked();
        let data = switcher.update(cx, |switcher, cx| {
            switcher.sync(cx);
            switcher.data(cx)
        });
        cx.run_until_parked();
        let data = if data.loaded {
            data
        } else {
            switcher.read_with(cx, |switcher, cx| switcher.data(cx))
        };
        assert!(data.loaded);
        assert_eq!(
            data.worktrees
                .iter()
                .map(|tree| tree.path.as_str())
                .collect::<Vec<_>>(),
            ["/picker-a", "/picker-b"]
        );
        assert_eq!(WorktreeSwitcher::title(&data), "Workspace");

        switcher.update(cx, |switcher, cx| {
            switcher.toggle(cx);
            switcher.select(
                Some(WorktreeFocus {
                    path: "/picker-a".into(),
                    branch: Some("feature-a".into()),
                }),
                cx,
            );
            let data = switcher.data(cx);
            assert!(data.pending);
            assert!(data.focus.is_none());
            assert!(!switcher.open);
        });
        cx.run_until_parked();
        switcher.update(cx, |switcher, cx| {
            let data = switcher.data(cx);
            assert!(!data.pending);
            assert!(data.focus.is_none());
            assert_eq!(
                data.switch_error.as_deref(),
                Some("Working copies are not available.")
            );
            switcher.react(&data, cx);
            assert!(switcher.open);
            assert_eq!(WorktreeSwitcher::title(&data), "Workspace");
        });
    }
}
