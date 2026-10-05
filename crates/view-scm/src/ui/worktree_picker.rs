//! Port of src/features/source-control/ui/WorktreePicker.tsx: the
//! composer's working copy button. Its popover lists the repository's
//! worktrees, creates one, switches the branch in place (through
//! [`BranchPicker`]), or opens worktree management.

use std::rc::Rc;

use gpui::{
    App, AppContext as _, Context, Entity, EventEmitter, InteractiveElement as _, IntoElement,
    MouseDownEvent, ParentElement as _, Render, ScrollHandle, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Task, Window, div,
    prelude::FluentBuilder as _,
};
use gpui_component::input::{Escape, InputEvent, InputState, MoveDown, MoveUp};
use monocode_engine::projects::{GitStatus, GitWatch, WatchKind};
use monocode_ui::widgets::{PopoverSide, popover_frame, tooltip};
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

use crate::git::Worktree;
use crate::model::worktrees::{
    active_worktree_index, filter_worktrees, worktree_row_text, worktree_trigger_label,
};
use crate::paths::{path_key, pretty_cwd};
use crate::scm::Scm;
use crate::ui::branch_picker::{BranchPicker, BranchPickerEvent};
use crate::ui::common::{
    BoundsCell, PopoverPlacement, anchored_popover, bare_input, contains, git_picker_trigger,
    spin_icon, track_bounds,
};
use crate::ui::dialogs::create_worktree::{CreateWorktreeDialog, CreateWorktreeEvent};

/// `onSelect`: move the session to a working copy. An error stays in the
/// popover.
pub type SelectWorktree = Rc<dyn Fn(Worktree, &mut Window, &mut App) -> Task<Result<(), String>>>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorktreePickerEvent {
    /// `onBranchChange`.
    BranchChanged,
    /// `onManage`: open the worktrees settings page.
    Manage,
    /// `onClose`.
    Close,
}

pub struct WorktreePicker {
    scm: Scm,
    cwd: String,
    execution_cwd: String,
    enabled: bool,
    opens_new_session: bool,
    worktree_removed: bool,
    can_manage: bool,
    on_select: SelectWorktree,
    open: bool,
    branch_picker: Option<Entity<BranchPicker>>,
    creating: Option<Entity<CreateWorktreeDialog>>,
    error: Option<String>,
    busy: bool,
    query: Entity<InputState>,
    active_path: Option<String>,
    branch_status: Option<Entity<GitStatus>>,
    _branch_watch: Option<GitWatch>,
    tree_status: Option<Entity<GitStatus>>,
    tree_watch: Option<GitWatch>,
    scroll: ScrollHandle,
    trigger_bounds: BoundsCell,
    task: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<WorktreePickerEvent> for WorktreePicker {}

impl WorktreePicker {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        scm: Scm,
        cwd: impl Into<String>,
        execution_cwd: impl Into<String>,
        opens_new_session: bool,
        worktree_removed: bool,
        on_select: SelectWorktree,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let cwd = cwd.into();
        let execution_cwd = execution_cwd.into();
        let query = cx.new(|cx| InputState::new(window, cx).placeholder("Search working copies…"));
        let mut subscriptions =
            vec![
                cx.subscribe_in(&query, window, |this, _, event: &InputEvent, window, cx| {
                    match event {
                        InputEvent::Change => {
                            this.active_path = None;
                            cx.notify();
                        }
                        InputEvent::PressEnter { .. } => this.enter(window, cx),
                        _ => {}
                    }
                }),
            ];
        let valid = !cwd.is_empty() && cwd != "~";
        let branch_cwd = if worktree_removed {
            cwd.clone()
        } else {
            execution_cwd.clone()
        };
        let branch_status = valid.then(|| scm.status(&branch_cwd, cx));
        let branch_watch = branch_status
            .as_ref()
            .map(|status| status.update(cx, |status, cx| status.watch(WatchKind::Branches, cx)));
        let tree_status = valid.then(|| scm.status(&cwd, cx));
        for status in branch_status.iter().chain(tree_status.iter()) {
            subscriptions.push(cx.observe(status, |this, _, cx| {
                this.update_watch(cx);
                cx.notify();
            }));
        }
        let mut this = Self {
            scm,
            cwd,
            execution_cwd,
            enabled: true,
            opens_new_session,
            worktree_removed,
            can_manage: false,
            on_select,
            open: false,
            branch_picker: None,
            creating: None,
            error: None,
            busy: false,
            query,
            active_path: None,
            branch_status,
            _branch_watch: branch_watch,
            tree_status,
            tree_watch: None,
            scroll: ScrollHandle::new(),
            trigger_bounds: BoundsCell::default(),
            task: None,
            _subscriptions: subscriptions,
        };
        this.update_watch(cx);
        this
    }

    /// Show "Manage worktrees…" (`onManage`).
    pub fn set_can_manage(&mut self, can_manage: bool) {
        self.can_manage = can_manage;
    }

    pub fn set_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.enabled = enabled;
        if !enabled {
            self.open = false;
            self.creating = None;
            self.branch_picker = None;
        }
        self.update_watch(cx);
        cx.notify();
    }

    fn current_branch(&self, cx: &App) -> (Option<String>, bool, bool) {
        match &self.branch_status {
            Some(status) => {
                let state = status.read(cx).branches_state();
                let current = state.branches.as_ref().and_then(|b| b.current.clone());
                let detached = state.branches.as_ref().is_some_and(|b| b.detached);
                (current, detached, state.settled)
            }
            None => (None, false, false),
        }
    }

    fn in_worktree(&self) -> bool {
        path_key(&self.cwd) != path_key(&self.execution_cwd)
    }

    /// `useProjectWorktrees(cwd, enabled && (worktreeRemoved || branch || inWorktree))`.
    fn update_watch(&mut self, cx: &mut Context<Self>) {
        let (current, _, _) = self.current_branch(cx);
        let want =
            self.enabled && (self.worktree_removed || current.is_some() || self.in_worktree());
        match (&self.tree_status, want) {
            (Some(status), true) if self.tree_watch.is_none() => {
                self.tree_watch =
                    Some(status.update(cx, |status, cx| status.watch(WatchKind::Worktrees, cx)));
            }
            (_, false) => self.tree_watch = None,
            _ => {}
        }
    }

    fn snapshot(&self, cx: &App) -> (Option<Vec<Worktree>>, Option<String>) {
        if self.tree_watch.is_none() {
            return (None, None);
        }
        match &self.tree_status {
            Some(status) => {
                let snapshot = status.read(cx).worktrees();
                (
                    snapshot.data.as_ref().map(|data| data.worktrees.clone()),
                    snapshot.error.clone(),
                )
            }
            None => (None, None),
        }
    }

    pub fn default_root(&self, cx: &App) -> Option<String> {
        self.tree_status.as_ref().and_then(|status| {
            status
                .read(cx)
                .worktrees()
                .data
                .as_ref()
                .map(|d| d.default_root.clone())
        })
    }

    // Reading, for tests.

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn query_input(&self) -> &Entity<InputState> {
        &self.query
    }

    pub fn branch_picker(&self) -> Option<&Entity<BranchPicker>> {
        self.branch_picker.as_ref()
    }

    pub fn creating(&self) -> Option<&Entity<CreateWorktreeDialog>> {
        self.creating.as_ref()
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// The filtered rows.
    pub fn rows(&self, cx: &App) -> Vec<Worktree> {
        let (data, _) = self.snapshot(cx);
        let query = self.query.read(cx).value().to_string();
        let trees = data.unwrap_or_default();
        filter_worktrees(&trees, &query)
            .into_iter()
            .cloned()
            .collect()
    }

    /// The highlighted row's index.
    pub fn active(&self, cx: &App) -> usize {
        let rows = self.rows(cx);
        let refs: Vec<&Worktree> = rows.iter().collect();
        active_worktree_index(&refs, self.active_path.as_deref(), &self.execution_cwd)
    }

    /// The line above the list, if any.
    pub fn notice(&self) -> Option<&'static str> {
        if self.worktree_removed {
            Some("This session’s worktree was deleted. Select a working copy to continue.")
        } else if self.opens_new_session {
            Some("Another working copy opens a new session.")
        } else {
            None
        }
    }

    /// Whether "Switch branch in this working copy…" is available.
    pub fn can_switch_branch(&self) -> bool {
        !self.busy && !self.worktree_removed
    }

    pub fn trigger_enabled(&self, cx: &App) -> bool {
        !self.trigger_disabled(cx)
    }

    /// Whether a row is the session's current working copy.
    pub fn is_current(&self, tree: &Worktree) -> bool {
        !self.worktree_removed && path_key(&tree.path) == path_key(&self.execution_cwd)
    }

    pub fn trigger_label(&self, cx: &App) -> String {
        let (current, detached, settled) = self.current_branch(cx);
        worktree_trigger_label(
            self.worktree_removed,
            current.as_deref(),
            detached,
            settled,
            self.in_worktree(),
        )
    }

    fn trigger_disabled(&self, cx: &App) -> bool {
        let (current, _, _) = self.current_branch(cx);
        !self.enabled || (!self.worktree_removed && current.is_none() && !self.in_worktree())
    }

    // Actions.

    pub fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open = false;
        self.query
            .update(cx, |state, cx| state.set_value("", window, cx));
        self.active_path = None;
        cx.emit(WorktreePickerEvent::Close);
        cx.notify();
    }

    /// The trigger's click.
    pub fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.trigger_disabled(cx) {
            return;
        }
        self.error = None;
        self.query
            .update(cx, |state, cx| state.set_value("", window, cx));
        self.active_path = None;
        if !self.open {
            self.refresh(cx);
            self.open = true;
            self.query.update(cx, |state, cx| state.focus(window, cx));
        } else {
            self.open = false;
        }
        cx.notify();
    }

    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.tree_watch.is_none() {
            return;
        }
        if let Some(status) = &self.tree_status {
            let load = status.update(cx, |status, cx| status.refresh_worktrees(cx));
            cx.spawn(async move |_, _| {
                load.await;
            })
            .detach();
        }
    }

    pub fn select(&mut self, tree: Worktree, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy || tree.missing {
            return;
        }
        self.busy = true;
        self.error = None;
        let task = (self.on_select)(tree, window, cx);
        self.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.busy = false;
                match result {
                    Ok(()) => this.dismiss(window, cx),
                    Err(error) => {
                        this.error = Some(error);
                        cx.notify();
                    }
                }
            });
        }));
        cx.notify();
    }

    /// Arrow keys in the search.
    pub fn move_active(&mut self, delta: isize, cx: &mut Context<Self>) {
        let rows = self.rows(cx);
        if rows.is_empty() {
            return;
        }
        let active = self.active(cx) as isize;
        let next = (active + delta).clamp(0, rows.len() as isize - 1) as usize;
        self.active_path = Some(rows[next].path.clone());
        self.scroll.scroll_to_item(next);
        cx.notify();
    }

    /// Enter in the search: select the highlighted row.
    pub fn enter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rows = self.rows(cx);
        if let Some(tree) = rows.get(self.active(cx)).cloned() {
            self.select(tree, window, cx);
        }
    }

    pub fn start_create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open = false;
        let base = if self.worktree_removed {
            self.cwd.clone()
        } else {
            self.execution_cwd.clone()
        };
        let root = self.default_root(cx);
        let dialog = cx.new(|cx| {
            CreateWorktreeDialog::new(self.scm.clone(), self.cwd.clone(), base, root, window, cx)
        });
        self._subscriptions.push(cx.subscribe_in(
            &dialog,
            window,
            |this, _, event: &CreateWorktreeEvent, window, cx| match event {
                CreateWorktreeEvent::Created(tree) => {
                    this.creating = None;
                    this.open = true;
                    this.select(tree.clone(), window, cx);
                }
                CreateWorktreeEvent::Cancel => {
                    this.creating = None;
                    cx.emit(WorktreePickerEvent::Close);
                    cx.notify();
                }
            },
        ));
        self.creating = Some(dialog);
        cx.notify();
    }

    pub fn switch_branch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy || self.worktree_removed {
            return;
        }
        self.open = false;
        let in_worktree = self.in_worktree();
        let enabled = self.enabled;
        let picker = cx.new(|cx| {
            let mut picker = BranchPicker::new(
                self.scm.clone(),
                self.execution_cwd.clone(),
                None,
                window,
                cx,
            );
            picker.set_worktree(in_worktree, cx);
            picker.set_enabled(enabled, window, cx);
            picker.open_popover(window, cx);
            picker
        });
        self._subscriptions.push(cx.subscribe_in(
            &picker,
            window,
            |this, _, event: &BranchPickerEvent, _, cx| match event {
                BranchPickerEvent::Dismiss => {
                    this.branch_picker = None;
                    cx.notify();
                }
                BranchPickerEvent::Changed => cx.emit(WorktreePickerEvent::BranchChanged),
                BranchPickerEvent::Close => {
                    this.branch_picker = None;
                    cx.emit(WorktreePickerEvent::Close);
                    cx.notify();
                }
                BranchPickerEvent::OpenChanged(_) => {}
            },
        ));
        self.branch_picker = Some(picker);
        cx.notify();
    }

    pub fn manage(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.dismiss(window, cx);
        cx.emit(WorktreePickerEvent::Manage);
    }
}

impl Render for WorktreePicker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(picker) = &self.branch_picker {
            return div()
                .flex()
                .min_w_0()
                .child(picker.clone())
                .into_any_element();
        }
        let theme = Theme::of(cx).clone();
        let c = theme.colors;
        let title = if self.worktree_removed {
            "Select a branch or worktree to continue this session".to_string()
        } else {
            format!("Working copy: {}", pretty_cwd(&self.execution_cwd))
        };
        let mut root = div()
            .relative()
            .flex()
            .min_w_0()
            .flex_shrink(1.)
            .child(track_bounds(&self.trigger_bounds))
            .child(
                git_picker_trigger("worktree-trigger", self.trigger_label(cx))
                    .title(title)
                    .worktree(!self.worktree_removed && self.in_worktree())
                    .expanded(self.open)
                    .disabled(self.trigger_disabled(cx))
                    .on_click(cx.listener(|this, _, window, cx| this.toggle(window, cx))),
            );
        if let Some(dialog) = &self.creating {
            root = root.child(dialog.clone());
        }
        if !self.open {
            return root.into_any_element();
        }
        let (data, load_error) = self.snapshot(cx);
        let rows = self.rows(cx);
        let active = self.active(cx);
        let mut body = div()
            .id("working-copies")
            .flex()
            .flex_col()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .p(u(4.));
        if data.is_none() && load_error.is_none() {
            body = body.child(
                div()
                    .flex()
                    .items_center()
                    .gap(u(8.))
                    .p(u(8.))
                    .text_px(12.)
                    .text_color(theme.content(0.50))
                    .child(spin_icon("worktrees-loading", 14., theme.content(0.50)))
                    .child("Loading working copies…"),
            );
        }
        for (index, tree) in rows.iter().enumerate() {
            let (title, detail) = worktree_row_text(tree);
            let selected = self.is_current(tree);
            let disabled = self.busy || tree.missing;
            let mut row = div()
                .id(SharedString::from(format!("tree-{}", tree.path)))
                .flex()
                .flex_none()
                .w_full()
                .items_center()
                .gap(u(8.))
                .rounded(u(6.))
                .px(u(8.))
                .py(u(8.))
                .tooltip(tooltip(tree.path.clone()));
            row = if active == index {
                row.bg(c.selection)
            } else {
                row.hover(|s| s.bg(theme.content(0.05)))
            };
            if disabled {
                row = row.opacity(0.4);
            } else {
                let picked = tree.clone();
                row =
                    row.on_click(cx.listener(move |this, _, window, cx| {
                        this.select(picked.clone(), window, cx)
                    }));
            }
            let path = tree.path.clone();
            row = row
                .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                    if *hovered && this.active_path.as_deref() != Some(path.as_str()) {
                        this.active_path = Some(path.clone());
                        cx.notify();
                    }
                }))
                .child(
                    icon(if tree.is_main {
                        IconName::GitBranch
                    } else {
                        IconName::FolderTree
                    })
                    .size(u(14.))
                    .text_color(theme.content(0.50)),
                )
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .child(
                            div()
                                .truncate()
                                .text_px(12.)
                                .text_color(c.content)
                                .child(title),
                        )
                        .child(
                            div()
                                .truncate()
                                .text_px(10.)
                                .text_color(theme.content(0.40))
                                .child(detail),
                        ),
                )
                .when(selected, |el| {
                    el.child(icon(IconName::Check).size(u(14.)).text_color(c.content))
                });
            body = body.child(row);
        }
        if data.is_some() && rows.is_empty() {
            body = body.child(
                div()
                    .p(u(8.))
                    .text_px(12.)
                    .text_color(theme.content(0.45))
                    .child("No matching working copies"),
            );
        }
        if let Some(error) = self.error.clone().or(load_error) {
            body = body.child(
                div()
                    .px(u(8.))
                    .py(u(8.))
                    .text_px(11.)
                    .text_color(c.danger)
                    .child(error),
            );
        }
        let footer_row = |id: &'static str,
                          glyph: IconName,
                          label: &'static str,
                          muted: bool,
                          disabled: bool| {
            let ink = if muted {
                theme.content(0.55)
            } else {
                c.content
            };
            let mut el = div()
                .id(id)
                .flex()
                .w_full()
                .items_center()
                .gap(u(8.))
                .rounded(u(6.))
                .px(u(8.))
                .py(u(8.))
                .text_color(ink)
                .child(icon(glyph).size(u(14.)).text_color(ink))
                .child(label);
            if disabled {
                el = el.opacity(0.4);
            } else {
                el = el.hover(|s| s.bg(theme.content(0.08)));
            }
            el
        };
        let mut footer = div()
            .flex_none()
            .border_t_1()
            .border_color(c.stroke)
            .p(u(4.))
            .text_px(12.)
            .child(
                footer_row(
                    "create-worktree",
                    IconName::Plus,
                    "Create worktree…",
                    false,
                    self.busy,
                )
                .when(!self.busy, |el| {
                    el.on_click(cx.listener(|this, _, window, cx| this.start_create(window, cx)))
                }),
            )
            .child(
                footer_row(
                    "switch-branch",
                    IconName::GitBranch,
                    "Switch branch in this working copy…",
                    true,
                    self.busy || self.worktree_removed,
                )
                .when(!self.busy && !self.worktree_removed, |el| {
                    el.on_click(cx.listener(|this, _, window, cx| this.switch_branch(window, cx)))
                }),
            );
        if self.can_manage {
            footer = footer.child(
                footer_row(
                    "manage-worktrees",
                    IconName::Settings,
                    "Manage worktrees…",
                    true,
                    self.busy,
                )
                .when(!self.busy, |el| {
                    el.on_click(cx.listener(|this, _, window, cx| this.manage(window, cx)))
                }),
            );
        }
        let mut content = div().flex().flex_col().max_h(u(398.)).child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(u(8.))
                .border_b_1()
                .border_color(c.stroke)
                .px(u(12.))
                .py(u(8.))
                .child(
                    icon(IconName::Search)
                        .size(u(14.))
                        .text_color(theme.content(0.40)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(bare_input(&self.query, 12., cx)),
                ),
        );
        if self.worktree_removed {
            content = content.child(
                div()
                    .flex_none()
                    .px(u(12.))
                    .pt(u(8.))
                    .pb(u(4.))
                    .text_px(11.)
                    .text_color(theme.content(0.50))
                    .child(
                        "This session’s worktree was deleted. Select a working copy to continue.",
                    ),
            );
        }
        if self.opens_new_session && !self.worktree_removed {
            content = content.child(
                div()
                    .flex_none()
                    .px(u(12.))
                    .pt(u(8.))
                    .pb(u(4.))
                    .text_px(11.)
                    .text_color(theme.content(0.50))
                    .child("Another working copy opens a new session."),
            );
        }
        content = content.child(body).child(footer);
        let frame = div()
            .id("worktree-picker")
            .occlude()
            .capture_action(cx.listener(|this, _: &MoveUp, _, cx| this.move_active(-1, cx)))
            .capture_action(cx.listener(|this, _: &MoveDown, _, cx| this.move_active(1, cx)))
            .capture_action(cx.listener(|this, _: &Escape, window, cx| {
                if !this.busy {
                    this.dismiss(window, cx);
                }
            }))
            .on_mouse_down_out(cx.listener(|this, event: &MouseDownEvent, window, cx| {
                if !this.busy && !contains(&this.trigger_bounds, event.position) {
                    this.dismiss(window, cx);
                }
            }))
            .child(
                popover_frame("worktree-picker-frame")
                    .side(PopoverSide::Top)
                    .width(320.)
                    .child(content),
            );
        root.child(anchored_popover(
            PopoverPlacement::TopStart,
            6.,
            theme.layer.popover,
            frame,
            window,
        ))
        .into_any_element()
    }
}
