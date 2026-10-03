//! Port of src/features/source-control/ui/BranchPicker.tsx: the composer's
//! branch button and its search, checkout, and create popover, with the
//! dialogs for a checkout git refused.

use gpui::{
    AppContext as _, Context, Entity, EventEmitter, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, ScrollHandle, SharedString, StatefulInteractiveElement as _,
    Styled as _, Subscription, Task, Window, div, prelude::FluentBuilder as _,
};
use gpui_component::input::{Escape, InputEvent, InputState, MoveDown, MoveUp};
use monocode_engine::projects::{GitStatus, GitWatch, WatchKind};
use monocode_ui::widgets::{PopoverSide, popover_frame};
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

use crate::git::{
    GitBranchEntry, GitBranches, is_checkout_blocked_by_changes,
    is_switch_blocked_by_running_sessions,
};
use crate::model::branches::{
    BranchTrigger, PendingSwitch, branch_rows, branch_trigger, create_row, create_row_label,
};
use crate::scm::Scm;
use crate::ui::common::{
    BoundsCell, PopoverPlacement, anchored_popover, bare_input, contains, git_picker_trigger,
    track_bounds, with_alpha,
};
use crate::ui::dialogs::create_branch::{CreateBranchDialog, CreateBranchEvent};
use crate::ui::dialogs::switch_branch::{SwitchBranchDialog, SwitchBranchEvent, SwitchBusy};
use crate::ui::dialogs::switch_while_running::{SwitchWhileRunningDialog, SwitchWhileRunningEvent};

const MENU_WIDTH: f32 = 280.;
const MENU_MIN_HEIGHT: f32 = 180.;
const MENU_MAX_HEIGHT: f32 = 280.;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BranchPickerEvent {
    /// `onDismiss`: the popover and dialogs closed.
    Dismiss,
    /// `onChange`: the branch changed.
    Changed,
    /// `onClose`: closed by Escape, a cancel, or a finished switch.
    Close,
    /// `onOpenChange`: whether any of the picker's surfaces is open.
    OpenChanged(bool),
}

/// Which way the popover opens from the trigger.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PickerSide {
    #[default]
    Top,
    Bottom,
}

struct Running {
    pending: PendingSwitch,
    dialog: Entity<SwitchWhileRunningDialog>,
    busy: bool,
}

struct Blocked {
    pending: PendingSwitch,
    dialog: Entity<SwitchBranchDialog>,
    busy: Option<SwitchBusy>,
}

pub struct BranchPicker {
    scm: Scm,
    cwd: String,
    branch: Option<String>,
    enabled: bool,
    worktree: bool,
    side: PickerSide,
    open: bool,
    query: Entity<InputState>,
    active: usize,
    busy: bool,
    error: Option<String>,
    creating: Option<Entity<CreateBranchDialog>>,
    blocked: Option<Blocked>,
    running: Option<Running>,
    status: Option<Entity<GitStatus>>,
    _watch: Option<GitWatch>,
    scroll: ScrollHandle,
    task: Option<Task<()>>,
    surface_open: bool,
    trigger_bounds: BoundsCell,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<BranchPickerEvent> for BranchPicker {}

impl BranchPicker {
    pub fn new(
        scm: Scm,
        cwd: impl Into<String>,
        branch: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let cwd = cwd.into();
        let query =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search or create a branch..."));
        let mut subscriptions =
            vec![
                cx.subscribe_in(&query, window, |this, _, event: &InputEvent, window, cx| {
                    match event {
                        InputEvent::Change => {
                            this.active = 0;
                            this.error = None;
                            cx.notify();
                        }
                        InputEvent::PressEnter { .. } => this.enter(window, cx),
                        _ => {}
                    }
                }),
            ];
        let in_project = !cwd.is_empty() && cwd != "~";
        let status = in_project.then(|| scm.status(&cwd, cx));
        let watch = status
            .as_ref()
            .map(|status| status.update(cx, |status, cx| status.watch(WatchKind::Branches, cx)));
        if let Some(status) = &status {
            subscriptions.push(cx.observe(status, |this, _, cx| {
                this.clamp_active(cx);
                cx.notify();
            }));
        }
        Self {
            scm,
            cwd,
            branch,
            enabled: true,
            worktree: false,
            side: PickerSide::Top,
            open: false,
            query,
            active: 0,
            busy: false,
            error: None,
            creating: None,
            blocked: None,
            running: None,
            status,
            _watch: watch,
            scroll: ScrollHandle::new(),
            task: None,
            surface_open: false,
            trigger_bounds: BoundsCell::default(),
            _subscriptions: subscriptions,
        }
    }

    /// Show the `Worktree` tag on the trigger.
    pub fn set_worktree(&mut self, worktree: bool, cx: &mut Context<Self>) {
        self.worktree = worktree;
        cx.notify();
    }

    pub fn set_side(&mut self, side: PickerSide) {
        self.side = side;
    }

    pub fn set_branch(&mut self, branch: Option<String>, cx: &mut Context<Self>) {
        self.branch = branch;
        cx.notify();
    }

    pub fn set_enabled(&mut self, enabled: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.enabled == enabled {
            return;
        }
        self.enabled = enabled;
        if !enabled {
            self.reset(window, cx);
        }
        cx.notify();
    }

    // Reading.

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn query_input(&self) -> &Entity<InputState> {
        &self.query
    }

    pub fn creating_dialog(&self) -> Option<&Entity<CreateBranchDialog>> {
        self.creating.as_ref()
    }

    pub fn blocked_dialog(&self) -> Option<&Entity<SwitchBranchDialog>> {
        self.blocked.as_ref().map(|b| &b.dialog)
    }

    pub fn running_dialog(&self) -> Option<&Entity<SwitchWhileRunningDialog>> {
        self.running.as_ref().map(|r| &r.dialog)
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn active(&self) -> usize {
        self.active
    }

    fn branches_state(&self, cx: &gpui::App) -> (Option<GitBranches>, bool) {
        match &self.status {
            Some(status) => {
                let state = status.read(cx).branches_state();
                (state.branches.clone(), state.settled)
            }
            None => (None, false),
        }
    }

    pub fn query(&self, cx: &gpui::App) -> String {
        self.query.read(cx).value().to_string()
    }

    /// The listed branches for the current query.
    pub fn rows(&self, cx: &gpui::App) -> Vec<GitBranchEntry> {
        let (branches, _) = self.branches_state(cx);
        branch_rows(branches.as_ref(), self.branch.as_deref(), &self.query(cx))
    }

    /// The fixed create action's name, or `None` when the name is taken.
    pub fn create_name(&self, cx: &gpui::App) -> Option<String> {
        let (branches, _) = self.branches_state(cx);
        create_row(branches.as_ref(), &self.query(cx))
    }

    pub fn trigger(&self, cx: &gpui::App) -> BranchTrigger {
        let (branches, settled) = self.branches_state(cx);
        branch_trigger(
            &self.cwd,
            self.branch.as_deref(),
            branches.as_ref(),
            settled,
            self.enabled,
        )
    }

    fn clamp_active(&mut self, cx: &gpui::App) {
        let len = self.rows(cx).len();
        self.active = if len == 0 {
            0
        } else {
            self.active.min(len - 1)
        };
    }

    // Opening and closing.

    fn report_open(&mut self, cx: &mut Context<Self>) {
        let open = self.open
            || self.creating.is_some()
            || self.blocked.is_some()
            || self.running.is_some();
        if open != self.surface_open {
            self.surface_open = open;
            cx.emit(BranchPickerEvent::OpenChanged(open));
        }
    }

    fn clear_query(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.query
            .update(cx, |state, cx| state.set_value("", window, cx));
    }

    fn reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open = false;
        self.creating = None;
        self.clear_query(window, cx);
        self.error = None;
        self.busy = false;
        self.blocked = None;
        self.running = None;
        self.task = None;
        self.report_open(cx);
    }

    /// `dismiss(restore)`.
    pub fn dismiss(&mut self, restore: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.reset(window, cx);
        cx.emit(BranchPickerEvent::Dismiss);
        if restore {
            cx.emit(BranchPickerEvent::Close);
        }
        cx.notify();
    }

    /// The trigger's click.
    pub fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.trigger(cx).interactive || self.blocked.is_some() {
            return;
        }
        if self.open {
            self.dismiss(true, window, cx);
            return;
        }
        self.open_popover(window, cx);
    }

    /// Open the popover and focus its search (`initialOpen`).
    pub fn open_popover(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open = true;
        self.clear_query(window, cx);
        self.error = None;
        self.active = 0;
        self.query.update(cx, |state, cx| state.focus(window, cx));
        self.report_open(cx);
        cx.notify();
    }

    /// Arrow keys in the search.
    pub fn move_active(&mut self, delta: isize, cx: &mut Context<Self>) {
        let len = self.rows(cx).len();
        if len == 0 {
            return;
        }
        let next = (self.active as isize + delta).clamp(0, len as isize - 1) as usize;
        self.active = next;
        self.scroll.scroll_to_item(next);
        cx.notify();
    }

    /// Enter in the search: the highlighted branch, else the create action.
    pub fn enter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.open {
            return;
        }
        let rows = self.rows(cx);
        if let Some(row) = rows.get(self.active).cloned() {
            self.pick_branch(row, window, cx);
            return;
        }
        if let Some(name) = self.create_name(cx)
            && !name.is_empty()
        {
            self.pick_create(name, window, cx);
        }
    }

    pub fn pick_branch(
        &mut self,
        row: GitBranchEntry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if row.current {
            self.dismiss(true, window, cx);
            return;
        }
        self.run(
            PendingSwitch::Checkout {
                name: row.name,
                remote: row.remote,
                force: false,
            },
            true,
            window,
            cx,
        );
    }

    /// The create action: an empty name asks for one in a dialog.
    pub fn pick_create(&mut self, name: String, window: &mut Window, cx: &mut Context<Self>) {
        if name.is_empty() {
            self.open = false;
            self.clear_query(window, cx);
            self.error = None;
            let dialog = cx.new(|cx| CreateBranchDialog::new(window, cx));
            self._subscriptions.push(cx.subscribe_in(
                &dialog,
                window,
                |this, _, event: &CreateBranchEvent, window, cx| match event {
                    CreateBranchEvent::Create(name) => this.run(
                        PendingSwitch::Create {
                            name: name.clone(),
                            force: false,
                        },
                        false,
                        window,
                        cx,
                    ),
                    CreateBranchEvent::Cancel => {
                        if this.busy {
                            return;
                        }
                        this.creating = None;
                        this.error = None;
                        this.report_open(cx);
                        cx.emit(BranchPickerEvent::Close);
                        cx.notify();
                    }
                },
            ));
            self.creating = Some(dialog);
            self.report_open(cx);
            cx.notify();
            return;
        }
        self.run(
            PendingSwitch::Create { name, force: false },
            true,
            window,
            cx,
        );
    }

    fn sync_create_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(dialog) = &self.creating {
            let (busy, error) = (self.busy, self.error.clone());
            dialog.update(cx, |dialog, cx| dialog.set_state(busy, error, window, cx));
        }
    }

    fn apply_switch(&self, pending: PendingSwitch, cx: &gpui::App) -> Task<Result<String, String>> {
        let cwd = self.cwd.clone();
        self.scm.run(cx, move |git| match pending {
            PendingSwitch::Create { name, force } => git.git_create_branch(&cwd, &name, force),
            PendingSwitch::Checkout {
                name,
                remote,
                force,
            } => git.git_checkout(&cwd, &name, remote.as_deref(), force),
        })
    }

    fn finish_switch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.scm.notify_git_changed(cx);
        cx.emit(BranchPickerEvent::Changed);
        self.dismiss(true, window, cx);
    }

    /// `blockOnChanges`.
    fn block_on_changes(
        &mut self,
        pending: PendingSwitch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open = false;
        self.creating = None;
        self.clear_query(window, cx);
        self.error = None;
        self.busy = false;
        self.running = None;
        let dialog = cx.new(|cx| {
            SwitchBranchDialog::new(
                self.scm.clone(),
                self.cwd.clone(),
                pending.name(),
                pending.creating(),
                window,
                cx,
            )
        });
        self._subscriptions.push(cx.subscribe_in(
            &dialog,
            window,
            |this, _, event: &SwitchBranchEvent, window, cx| match event {
                SwitchBranchEvent::Stash => {
                    this.resolve_blocked(SwitchBusy::Stash, None, window, cx)
                }
                SwitchBranchEvent::Commit(message) => {
                    this.resolve_blocked(SwitchBusy::Commit, Some(message.clone()), window, cx)
                }
                SwitchBranchEvent::Cancel => {
                    if this.blocked.as_ref().is_some_and(|b| b.busy.is_some()) {
                        return;
                    }
                    this.blocked = None;
                    this.report_open(cx);
                    cx.emit(BranchPickerEvent::Close);
                    cx.notify();
                }
            },
        ));
        self.blocked = Some(Blocked {
            pending,
            dialog,
            busy: None,
        });
        self.report_open(cx);
        cx.notify();
    }

    /// `run`.
    fn run(
        &mut self,
        pending: PendingSwitch,
        from_picker: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.busy || self.blocked.is_some() || self.running.is_some() {
            return;
        }
        self.busy = true;
        self.error = None;
        self.sync_create_dialog(window, cx);
        let call = self.apply_switch(pending.clone(), cx);
        self.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = call.await;
            let _ = this.update_in(cx, |this, window, cx| match result {
                Ok(_) => this.finish_switch(window, cx),
                Err(message) => {
                    if is_switch_blocked_by_running_sessions(&message) {
                        this.open = false;
                        this.creating = None;
                        this.clear_query(window, cx);
                        this.error = None;
                        this.busy = false;
                        let dialog = cx.new(|_| {
                            SwitchWhileRunningDialog::new(
                                pending.name(),
                                pending.creating(),
                                message.clone(),
                            )
                        });
                        this._subscriptions.push(cx.subscribe_in(
                            &dialog,
                            window,
                            |this, _, event: &SwitchWhileRunningEvent, window, cx| match event {
                                SwitchWhileRunningEvent::Confirm => {
                                    this.confirm_running(window, cx)
                                }
                                SwitchWhileRunningEvent::Cancel => {
                                    if this.running.as_ref().is_some_and(|r| r.busy) {
                                        return;
                                    }
                                    this.running = None;
                                    this.report_open(cx);
                                    cx.emit(BranchPickerEvent::Close);
                                    cx.notify();
                                }
                            },
                        ));
                        this.running = Some(Running {
                            pending: pending.clone(),
                            dialog,
                            busy: false,
                        });
                        this.report_open(cx);
                        cx.notify();
                        return;
                    }
                    if is_checkout_blocked_by_changes(&message) {
                        this.block_on_changes(pending.clone(), window, cx);
                        return;
                    }
                    this.error = Some(message);
                    this.busy = false;
                    this.sync_create_dialog(window, cx);
                    if from_picker {
                        this.query.update(cx, |state, cx| state.focus(window, cx));
                    }
                    cx.notify();
                }
            });
        }));
        cx.notify();
    }

    /// `confirmRunning`: switch anyway.
    fn confirm_running(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(running) = &mut self.running else {
            return;
        };
        if running.busy {
            return;
        }
        running.busy = true;
        let pending = running.pending.forced();
        running
            .dialog
            .update(cx, |dialog, cx| dialog.set_state(true, None, cx));
        let call = self.apply_switch(pending.clone(), cx);
        self.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = call.await;
            let _ = this.update_in(cx, |this, window, cx| match result {
                Ok(_) => this.finish_switch(window, cx),
                Err(message) => {
                    if is_checkout_blocked_by_changes(&message) {
                        this.block_on_changes(pending.clone(), window, cx);
                        return;
                    }
                    if let Some(running) = &mut this.running {
                        running.busy = false;
                        running
                            .dialog
                            .update(cx, |dialog, cx| dialog.set_state(false, Some(message), cx));
                    }
                    cx.notify();
                }
            });
        }));
    }

    /// `resolveBlocked`: stash or commit, then switch.
    fn resolve_blocked(
        &mut self,
        kind: SwitchBusy,
        message: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(blocked) = &mut self.blocked else {
            return;
        };
        if blocked.busy.is_some() {
            return;
        }
        blocked.busy = Some(kind);
        blocked
            .dialog
            .update(cx, |dialog, cx| dialog.set_state(Some(kind), None, cx));
        let pending = blocked.pending.clone();
        let cwd = self.cwd.clone();
        let name = pending.name().to_string();
        let work = self.scm.run(cx, move |git| match kind {
            SwitchBusy::Stash => {
                git.git_stash(&cwd, Some(&format!("WIP before switching to {name}")))
            }
            SwitchBusy::Commit => {
                git.git_stage_all(&cwd)?;
                git.git_commit(&cwd, message.as_deref().unwrap_or(""), false)
            }
        });
        let scm = self.scm.clone();
        let switch_cwd = self.cwd.clone();
        self.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = match work.await {
                Ok(()) => {
                    let pending = pending.clone();
                    cx.background_spawn(async move {
                        match pending {
                            PendingSwitch::Create { name, force } => {
                                scm.git.git_create_branch(&switch_cwd, &name, force)
                            }
                            PendingSwitch::Checkout {
                                name,
                                remote,
                                force,
                            } => scm
                                .git
                                .git_checkout(&switch_cwd, &name, remote.as_deref(), force),
                        }
                    })
                    .await
                }
                Err(error) => Err(error),
            };
            let _ = this.update_in(cx, |this, window, cx| match result {
                Ok(_) => this.finish_switch(window, cx),
                Err(error) => {
                    if let Some(blocked) = &mut this.blocked {
                        blocked.busy = None;
                        blocked
                            .dialog
                            .update(cx, |dialog, cx| dialog.set_state(None, Some(error), cx));
                    }
                    cx.notify();
                }
            });
        }));
    }
}

impl Render for BranchPicker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let c = theme.colors;
        let trigger = self.trigger(cx);
        let mut root = div()
            .relative()
            .flex()
            .min_w_0()
            .flex_shrink(1.)
            .child(track_bounds(&self.trigger_bounds))
            .child(
                git_picker_trigger("branch-trigger", trigger.label.clone())
                    .title(trigger.title.clone())
                    .loading(trigger.awaiting)
                    .worktree(self.worktree)
                    .expanded(self.open && !trigger.missing_git)
                    .disabled(!trigger.interactive)
                    .on_click(cx.listener(|this, _, window, cx| this.toggle(window, cx))),
            );
        if let Some(running) = &self.running {
            root = root.child(running.dialog.clone());
        }
        if let Some(blocked) = &self.blocked {
            root = root.child(blocked.dialog.clone());
        }
        if let Some(creating) = &self.creating {
            root = root.child(creating.clone());
        }
        if !self.open {
            return root;
        }
        let rows = self.rows(cx);
        let query = self.query(cx);
        let busy = self.busy;
        let search = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(u(8.))
            .border_b_1()
            .border_color(c.stroke)
            .px(u(12.))
            .py(u(10.))
            .child(
                icon(IconName::Search)
                    .size(u(14.))
                    .text_color(theme.content(0.50)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .when(busy, |el| el.opacity(0.6))
                    .child(bare_input(&self.query, 13., cx)),
            );
        let list = if rows.is_empty() {
            div()
                .flex_1()
                .min_h_0()
                .px(u(12.))
                .py(u(16.))
                .text_px(12.)
                .text_color(theme.content(0.50))
                .child(if query.trim().is_empty() {
                    "No branches"
                } else {
                    "No matching branches"
                })
                .into_any_element()
        } else {
            let mut list = div()
                .id("branches")
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(&self.scroll)
                .px(u(6.))
                .py(u(6.));
            for (index, row) in rows.into_iter().enumerate() {
                let highlighted = index == self.active;
                let selected = row.current;
                let key: SharedString =
                    format!("{}:{}", row.remote.as_deref().unwrap_or("local"), row.name).into();
                let mut item = div()
                    .id(key)
                    .flex()
                    .flex_none()
                    .h(u(32.))
                    .w_full()
                    .items_center()
                    .gap(u(8.))
                    .rounded(u(8.))
                    .px(u(8.))
                    .text_px(13.)
                    .text_color(c.content);
                if highlighted || selected {
                    item = item.bg(c.selection);
                } else {
                    item = item.hover(|s| s.bg(theme.content(0.05)));
                }
                if busy {
                    item = item.opacity(0.6);
                }
                item = item.child(if selected {
                    icon(IconName::Check).size(u(14.)).text_color(c.content)
                } else {
                    icon(IconName::GitBranch)
                        .size(u(14.))
                        .text_color(theme.content(0.50))
                });
                item = item.child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .truncate()
                        .when(selected, |el| el.medium())
                        .child(row.name.clone()),
                );
                if let Some(remote) = &row.remote {
                    item = item.child(
                        div()
                            .flex_none()
                            .rounded(u(4.))
                            .bg(theme.content(0.06))
                            .px(u(6.))
                            .py(u(2.))
                            .text_px(10.)
                            .text_color(theme.content(0.40))
                            .child(remote.clone()),
                    );
                }
                if !busy {
                    let picked = row.clone();
                    item = item
                        .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                            if *hovered && this.active != index {
                                this.active = index;
                                cx.notify();
                            }
                        }))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.pick_branch(picked.clone(), window, cx)
                        }));
                }
                list = list.child(item);
            }
            list.into_any_element()
        };
        let mut content = div()
            .flex()
            .flex_col()
            .min_h(u(MENU_MIN_HEIGHT))
            .max_h(u(MENU_MAX_HEIGHT))
            .child(search)
            .child(list);
        if let Some(error) = &self.error {
            content = content.child(
                div()
                    .flex_none()
                    .max_h(u(64.))
                    .border_t_1()
                    .border_color(c.stroke)
                    .px(u(10.))
                    .py(u(8.))
                    .text_px(11.)
                    .line_height(u(16.))
                    .text_color(with_alpha(c.danger, 0.9))
                    .child(error.clone()),
            );
        }
        if let Some(name) = self.create_name(cx) {
            let label = create_row_label(&name);
            let mut button = div()
                .id("create-branch")
                .flex()
                .h(u(30.))
                .w_full()
                .items_center()
                .gap(u(10.))
                .rounded(u(8.))
                .px(u(10.))
                .text_px(13.)
                .text_color(theme.content(0.75))
                .child(
                    icon(IconName::Plus)
                        .size(u(16.))
                        .text_color(theme.content(0.75)),
                )
                .child(div().min_w_0().truncate().child(label));
            if busy {
                button = button.opacity(0.6);
            } else {
                button = button
                    .hover(|s| s.bg(theme.content(0.08)).text_color(c.content))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.pick_create(name.clone(), window, cx)
                    }));
            }
            content = content.child(
                div()
                    .flex_none()
                    .border_t_1()
                    .border_color(c.stroke)
                    .p(u(4.))
                    .px(u(6.))
                    .child(button),
            );
        }
        let side = match self.side {
            PickerSide::Top => PopoverSide::Top,
            PickerSide::Bottom => PopoverSide::Bottom,
        };
        let frame = div()
            .id("branch-picker")
            .occlude()
            .capture_action(cx.listener(|this, _: &MoveUp, _, cx| this.move_active(-1, cx)))
            .capture_action(cx.listener(|this, _: &MoveDown, _, cx| this.move_active(1, cx)))
            .capture_action(
                cx.listener(|this, _: &Escape, window, cx| this.dismiss(true, window, cx)),
            )
            .on_mouse_down_out(
                cx.listener(|this, event: &gpui::MouseDownEvent, window, cx| {
                    if this.open && !contains(&this.trigger_bounds, event.position) {
                        this.dismiss(false, window, cx);
                    }
                }),
            )
            .child(
                popover_frame("branch-picker-frame")
                    .side(side)
                    .width(MENU_WIDTH)
                    .child(content),
            );
        let placement = match self.side {
            PickerSide::Top => PopoverPlacement::TopStart,
            PickerSide::Bottom => PopoverPlacement::BottomStart,
        };
        root.child(anchored_popover(
            placement,
            6.,
            theme.layer.popover,
            frame,
            window,
        ))
    }
}
