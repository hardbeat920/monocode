//! Port of src/features/quick-composer/ui/QuickGitPopup.tsx and the parts of
//! BranchPicker.tsx, WorkspacePicker.tsx, CreateBranchDialog.tsx,
//! SwitchBranchDialog.tsx, and SwitchWhileRunningDialog.tsx it shows.
//!
//! The popup is its own panel, so the composer's frame and glass never grow.
//! A request names one picker: the working copy (current checkout, new
//! worktree, or an existing worktree), the new worktree's base branch, or
//! the branch of the current checkout. The popup finishes once, with or
//! without a new choice, and the composer applies the result.

use std::rc::Rc;
use std::time::Duration;

use gpui::{
    AnyElement, App, Context, Entity, EventEmitter, FocusHandle, Focusable, Hsla,
    InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Task, Window, canvas, div,
};
use monocode_core::session::WorkspaceMode;
use monocode_layout::paths::pretty_cwd;
use monocode_ui::widgets::{kbd, spinner};
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};
use monocode_view_composer::composer::prompt_input::{self, PromptInput, PromptInputEvent};

use crate::colors;
use crate::field::search_field;
use crate::host::QuickGitHost;
use crate::model::git_popup::{
    PendingSwitch, base_rows, blocked_message, branch_rows, create_label, create_row,
    existing_worktrees, is_checkout_blocked_by_changes, is_switch_blocked_by_running_sessions,
    running_sessions_message, worktree_title,
};
use crate::model::launch::{GitBranches, QuickGitKind, QuickGitRequest, QuickWorkspace, Worktree};

/// What the popup reports.
#[derive(Debug, Clone, PartialEq)]
pub enum QuickGitPopupEvent {
    /// `quick_git_complete(id, choice, restoreFocus: true)`. If completing
    /// fails, call [`QuickGitPopup::finish_failed`].
    Finish {
        id: String,
        choice: Option<QuickWorkspace>,
    },
    /// `quick_git_fit`: the content is this tall (CSS px, rounded up).
    Fit { id: String, height: u32 },
    /// `onShown`: a request arrived; the theme may have changed.
    Shown,
}

/// Where the branch picker stands.
#[derive(Default)]
enum BranchStage {
    #[default]
    List,
    /// `CreateBranchDialog`.
    Create { error: Option<String> },
    /// `SwitchBranchDialog`: git refused because local changes would be
    /// overwritten.
    Blocked {
        pending: PendingSwitch,
        busy: Option<&'static str>,
        error: Option<String>,
    },
    /// `SwitchWhileRunningDialog`.
    Running {
        pending: PendingSwitch,
        message: String,
        busy: bool,
        error: Option<String>,
    },
}

/// The existing worktree submenu.
#[derive(Default)]
struct WorktreeMenu {
    open: bool,
    data: Option<Result<Vec<Worktree>, String>>,
    busy_path: Option<String>,
    task: Option<Task<()>>,
}

pub struct QuickGitPopup {
    git: Rc<dyn QuickGitHost>,
    focus_handle: FocusHandle,
    request: Option<QuickGitRequest>,
    finished: bool,
    finish_error: Option<String>,

    branches: Option<GitBranches>,
    settled: bool,
    branches_cwd: String,
    branches_task: Option<Task<()>>,

    search: Entity<PromptInput>,
    name: Entity<PromptInput>,
    message: Entity<PromptInput>,
    active: usize,
    busy: bool,
    error: Option<String>,
    stage: BranchStage,
    switch_task: Option<Task<()>>,
    worktrees: WorktreeMenu,
    fit_sent: Option<u32>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<QuickGitPopupEvent> for QuickGitPopup {}

impl Focusable for QuickGitPopup {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl QuickGitPopup {
    pub fn new(git: Rc<dyn QuickGitHost>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = search_field("Search or create a branch...", window, cx);
        let name = search_field("feature/my-branch", window, cx);
        let message = search_field("Message (\u{2318}\u{21a9} to commit)", window, cx);
        let subscriptions = vec![
            cx.subscribe(&search, |this, _, event: &PromptInputEvent, cx| {
                if *event == PromptInputEvent::Changed {
                    this.active = 0;
                    this.error = None;
                    cx.notify();
                }
            }),
        ];
        Self {
            git,
            focus_handle: cx.focus_handle(),
            request: None,
            finished: false,
            finish_error: None,
            branches: None,
            settled: false,
            branches_cwd: String::new(),
            branches_task: None,
            search,
            name,
            message,
            active: 0,
            busy: false,
            error: None,
            stage: BranchStage::List,
            switch_task: None,
            worktrees: WorktreeMenu::default(),
            fit_sent: None,
            _subscriptions: subscriptions,
        }
    }

    /// The live request.
    pub fn request(&self) -> Option<&QuickGitRequest> {
        self.request.as_ref()
    }

    pub fn branches(&self) -> Option<&GitBranches> {
        self.branches.as_ref()
    }

    /// `waiting`: the first branch lookup has not answered with branches.
    pub fn waiting(&self) -> bool {
        !self.settled || self.branches.is_none()
    }

    pub fn search(&self) -> &Entity<PromptInput> {
        &self.search
    }

    pub fn name_input(&self) -> &Entity<PromptInput> {
        &self.name
    }

    pub fn is_creating(&self) -> bool {
        matches!(self.stage, BranchStage::Create { .. })
    }

    pub fn worktree_menu_open(&self) -> bool {
        self.worktrees.open
    }

    /// `QUICK_GIT_REQUEST`: show a request. A cold popup shows the branches
    /// the composer had at once and still refreshes them.
    pub fn open(&mut self, request: QuickGitRequest, window: &mut Window, cx: &mut Context<Self>) {
        cx.emit(QuickGitPopupEvent::Shown);
        let cwd = request.choice.git_cwd();
        self.finished = false;
        self.finish_error = None;
        self.busy = false;
        self.error = None;
        self.active = 0;
        self.stage = BranchStage::List;
        self.switch_task = None;
        self.worktrees = WorktreeMenu::default();
        self.fit_sent = None;
        if let Some(branches) = request.branches.clone() {
            self.branches = Some(branches);
            self.settled = true;
        } else if cwd != self.branches_cwd {
            self.branches = None;
            self.settled = false;
        }
        self.branches_cwd = cwd.clone();
        self.request = Some(request);
        for input in [&self.search, &self.name, &self.message] {
            input.update(cx, |input, cx| input.reset_text("", cx));
        }
        self.load_branches(cwd, cx);
        self.focus_field(window, cx);
        cx.notify();
    }

    fn load_branches(&mut self, cwd: String, cx: &mut Context<Self>) {
        if cwd.is_empty() {
            self.settled = true;
            return;
        }
        let task = self.git.branches(&cwd, cx);
        self.branches_task = Some(cx.spawn(async move |this, cx| {
            let branches = task.await;
            this.update(cx, |this, cx| {
                if this.branches_cwd != cwd {
                    return;
                }
                this.branches = branches;
                this.settled = true;
                this.branches_task = None;
                cx.notify();
            })
            .ok();
        }));
    }

    /// Focus the picker's field when the panel takes keys.
    pub fn focus_field(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let field = match (&self.stage, self.kind()) {
            (BranchStage::Create { .. }, _) => Some(&self.name),
            (BranchStage::Blocked { .. }, _) => Some(&self.message),
            (_, Some(QuickGitKind::Branch | QuickGitKind::Base)) => Some(&self.search),
            _ => None,
        };
        match field {
            Some(field) => {
                let field = field.clone();
                field.update(cx, |field, cx| field.focus(window, cx));
            }
            None => window.focus(&self.focus_handle, cx),
        }
    }

    fn kind(&self) -> Option<QuickGitKind> {
        self.request.as_ref().map(|request| request.kind)
    }

    fn choice(&self) -> Option<QuickWorkspace> {
        self.request.as_ref().map(|request| request.choice.clone())
    }

    fn cwd(&self) -> String {
        self.request
            .as_ref()
            .map(|request| request.choice.git_cwd())
            .unwrap_or_default()
    }

    /// `finish`: once per request.
    pub fn finish(&mut self, choice: Option<QuickWorkspace>, cx: &mut Context<Self>) {
        if self.finished {
            return;
        }
        let Some(id) = self.request.as_ref().map(|request| request.id.clone()) else {
            return;
        };
        self.finished = true;
        cx.emit(QuickGitPopupEvent::Finish { id, choice });
    }

    /// Completing failed: show why and allow another try.
    pub fn finish_failed(&mut self, error: String, cx: &mut Context<Self>) {
        self.finished = false;
        self.finish_error = Some(error);
        cx.notify();
    }

    // The branch picker.

    /// The branch rows for the current query.
    pub fn branch_rows(&self, cx: &App) -> Vec<crate::model::launch::GitBranchInfo> {
        branch_rows(self.branches.as_ref(), self.search.read(cx).text())
    }

    /// `pick` for a row index, or the create row when `index` is past the
    /// rows.
    pub fn pick_branch(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let rows = self.branch_rows(cx);
        if let Some(row) = rows.get(index) {
            if row.current {
                self.finish(None, cx);
                return;
            }
            self.run(
                PendingSwitch::Checkout {
                    name: row.name.clone(),
                    remote: row.remote.clone(),
                    force: false,
                },
                window,
                cx,
            );
            return;
        }
        let Some(name) = create_row(self.branches.as_ref(), self.search.read(cx).text()) else {
            return;
        };
        if name.is_empty() {
            self.stage = BranchStage::Create { error: None };
            self.error = None;
            self.focus_field(window, cx);
            cx.notify();
            return;
        }
        self.run(PendingSwitch::Create { name, force: false }, window, cx);
    }

    /// The create form's button.
    pub fn create_branch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.name.read(cx).text().trim().to_string();
        if name.is_empty() || self.busy {
            return;
        }
        self.run(PendingSwitch::Create { name, force: false }, window, cx);
    }

    fn apply_switch(&self, pending: &PendingSwitch, cx: &mut App) -> Task<Result<(), String>> {
        let cwd = self.cwd();
        match pending {
            PendingSwitch::Create { name, force } => self.git.create_branch(&cwd, name, *force, cx),
            PendingSwitch::Checkout {
                name,
                remote,
                force,
            } => self.git.checkout(&cwd, name, remote.as_deref(), *force, cx),
        }
    }

    /// `run`.
    fn run(&mut self, pending: PendingSwitch, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy
            || matches!(
                self.stage,
                BranchStage::Blocked { .. } | BranchStage::Running { .. }
            )
        {
            return;
        }
        self.busy = true;
        self.error = None;
        if let BranchStage::Create { error } = &mut self.stage {
            *error = None;
        }
        cx.notify();
        let task = self.apply_switch(&pending, cx);
        self.switch_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            this.update_in(cx, |this, window, cx| {
                this.switch_task = None;
                match result {
                    Ok(()) => this.finish_switch(cx),
                    Err(message) => this.switch_failed(pending, message, window, cx),
                }
            })
            .ok();
        }));
    }

    fn switch_failed(
        &mut self,
        pending: PendingSwitch,
        message: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.busy = false;
        if is_switch_blocked_by_running_sessions(&message) {
            self.stage = BranchStage::Running {
                pending,
                message,
                busy: false,
                error: None,
            };
        } else if is_checkout_blocked_by_changes(&message) {
            self.stage = BranchStage::Blocked {
                pending,
                busy: None,
                error: None,
            };
            self.message
                .update(cx, |message, cx| message.reset_text("", cx));
            self.focus_field(window, cx);
        } else if let BranchStage::Create { error } = &mut self.stage {
            *error = Some(message);
        } else {
            self.error = Some(message);
        }
        cx.notify();
    }

    /// `finishSwitch`.
    fn finish_switch(&mut self, cx: &mut Context<Self>) {
        self.busy = false;
        self.git.git_changed(cx);
        let choice = self.choice();
        self.finish(choice, cx);
        cx.notify();
    }

    /// `confirmRunning`: switch anyway.
    pub fn confirm_running(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let BranchStage::Running {
            pending,
            busy,
            error,
            ..
        } = &mut self.stage
        else {
            return;
        };
        if *busy {
            return;
        }
        *busy = true;
        *error = None;
        let pending = pending.forced();
        let task = self.apply_switch(&pending, cx);
        cx.notify();
        self.switch_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            this.update_in(cx, |this, window, cx| {
                this.switch_task = None;
                match result {
                    Ok(()) => this.finish_switch(cx),
                    Err(message) if is_checkout_blocked_by_changes(&message) => {
                        this.stage = BranchStage::Blocked {
                            pending,
                            busy: None,
                            error: None,
                        };
                        this.focus_field(window, cx);
                    }
                    Err(message) => {
                        if let BranchStage::Running { busy, error, .. } = &mut this.stage {
                            *busy = false;
                            *error = Some(message);
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        }));
    }

    /// `resolveBlocked`: stash or commit, then switch.
    pub fn resolve_blocked(&mut self, commit: bool, window: &mut Window, cx: &mut Context<Self>) {
        let message = self.message.read(cx).text().trim().to_string();
        let BranchStage::Blocked {
            pending,
            busy,
            error,
        } = &mut self.stage
        else {
            return;
        };
        if busy.is_some() || (commit && message.is_empty()) {
            return;
        }
        *busy = Some(if commit { "commit" } else { "stash" });
        *error = None;
        let pending = pending.clone();
        let cwd = self.cwd();
        let work = if commit {
            self.git.commit_all(&cwd, &message, cx)
        } else {
            let note = format!("WIP before switching to {}", pending.name());
            self.git.stash(&cwd, &note, cx)
        };
        cx.notify();
        self.switch_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = match work.await {
                Ok(()) => {
                    let task = this.update(cx, |this, cx| this.apply_switch(&pending, cx));
                    match task {
                        Ok(task) => task.await,
                        Err(err) => Err(err.to_string()),
                    }
                }
                Err(err) => Err(err),
            };
            this.update(cx, |this, cx| {
                this.switch_task = None;
                match result {
                    Ok(()) => this.finish_switch(cx),
                    Err(message) => {
                        if let BranchStage::Blocked { busy, error, .. } = &mut this.stage {
                            *busy = None;
                            *error = Some(message);
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        }));
    }

    /// Cancel in a dialog: `onClose`, which finishes the popup.
    fn cancel_dialog(&mut self, cx: &mut Context<Self>) {
        let busy = match &self.stage {
            BranchStage::Create { .. } => self.busy,
            BranchStage::Blocked { busy, .. } => busy.is_some(),
            BranchStage::Running { busy, .. } => *busy,
            BranchStage::List => false,
        };
        if busy {
            return;
        }
        self.stage = BranchStage::List;
        self.finish(None, cx);
    }

    // The working copy picker.

    /// `onModeChange`.
    pub fn choose_mode(&mut self, mode: WorkspaceMode, cx: &mut Context<Self>) {
        let Some(choice) = self.choice() else {
            return;
        };
        let base = (mode == WorkspaceMode::Worktree).then(|| self.effective_base());
        self.finish(
            Some(QuickWorkspace {
                cwd: choice.cwd,
                mode,
                base,
                tree: None,
            }),
            cx,
        );
    }

    /// `effectiveBase`.
    fn effective_base(&self) -> String {
        self.choice()
            .and_then(|choice| choice.base)
            .filter(|base| !base.is_empty())
            .or_else(|| {
                self.branches
                    .as_ref()
                    .and_then(|branches| branches.current.clone())
            })
            .unwrap_or_else(|| "HEAD".into())
    }

    /// `openWorktreeMenu`: list the project's worktrees.
    pub fn open_worktree_menu(&mut self, cx: &mut Context<Self>) {
        if self.worktrees.open {
            return;
        }
        self.worktrees.open = true;
        if self.worktrees.data.is_none() {
            let cwd = self.cwd();
            let task = self.git.worktrees(&cwd, cx);
            self.worktrees.task = Some(cx.spawn(async move |this, cx| {
                let listed = task.await;
                this.update(cx, |this, cx| {
                    this.worktrees.data = Some(listed);
                    this.worktrees.task = None;
                    cx.notify();
                })
                .ok();
            }));
        }
        cx.notify();
    }

    fn close_worktree_menu(&mut self, cx: &mut Context<Self>) {
        if self.worktrees.open {
            self.worktrees.open = false;
            cx.notify();
        }
    }

    /// `selectWorktree`.
    pub fn select_worktree(&mut self, tree: Worktree, cx: &mut Context<Self>) {
        if self.worktrees.busy_path.is_some() {
            return;
        }
        let Some(choice) = self.choice() else {
            return;
        };
        self.worktrees.busy_path = Some(tree.path.clone());
        self.finish(
            Some(QuickWorkspace {
                cwd: choice.cwd,
                mode: WorkspaceMode::Current,
                base: None,
                tree: Some(tree),
            }),
            cx,
        );
        cx.notify();
    }

    // The base picker.

    pub fn base_rows(&self, cx: &App) -> Vec<crate::model::launch::GitBranchInfo> {
        base_rows(
            self.branches
                .as_ref()
                .map(|branches| branches.branches.as_slice())
                .unwrap_or(&[]),
            self.search.read(cx).text(),
        )
    }

    /// `onBaseChange`.
    pub fn choose_base(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(row) = self.base_rows(cx).get(index).cloned() else {
            return;
        };
        let Some(mut choice) = self.choice() else {
            return;
        };
        choice.base = Some(row.reference());
        self.finish(Some(choice), cx);
    }

    // Keys.

    fn row_count(&self, cx: &App) -> usize {
        match self.kind() {
            Some(QuickGitKind::Base) => self.base_rows(cx).len(),
            _ => {
                self.branch_rows(cx).len()
                    + usize::from(
                        create_row(self.branches.as_ref(), self.search.read(cx).text()).is_some(),
                    )
            }
        }
    }

    fn step(&mut self, down: bool, cx: &mut Context<Self>) {
        let count = self.row_count(cx);
        if count == 0 {
            return;
        }
        self.active = if down {
            (self.active + 1).min(count - 1)
        } else {
            self.active.saturating_sub(1)
        };
        cx.notify();
    }

    fn enter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match (&self.stage, self.kind()) {
            (BranchStage::Create { .. }, _) => self.create_branch(window, cx),
            (BranchStage::Blocked { .. }, _) => {
                if window.modifiers().platform || window.modifiers().control {
                    self.resolve_blocked(true, window, cx);
                }
            }
            (_, Some(QuickGitKind::Base)) => {
                if self.active < self.base_rows(cx).len() {
                    self.choose_base(self.active, cx);
                }
            }
            (_, Some(QuickGitKind::Branch)) => {
                let count = self.branch_rows(cx).len();
                if self.active < count {
                    self.pick_branch(self.active, window, cx);
                } else if create_row(self.branches.as_ref(), self.search.read(cx).text())
                    .is_some_and(|name| !name.is_empty())
                {
                    self.pick_branch(count, window, cx);
                }
            }
            _ => {}
        }
    }

    /// Escape everywhere dismisses without a new choice; inside a dialog
    /// it cancels the dialog, which also finishes.
    pub fn escape(&mut self, cx: &mut Context<Self>) {
        match self.stage {
            BranchStage::List => self.finish(None, cx),
            _ => self.cancel_dialog(cx),
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key == "escape" && !event.keystroke.modifiers.modified() {
            cx.stop_propagation();
            self.escape(cx);
        }
    }
}

// Drawing.

fn row_base(id: SharedString, theme: &Theme) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex()
        .flex_none()
        .w_full()
        .items_center()
        .gap(u(8.))
        .rounded(u(theme.radius.lg))
        .px(u(8.))
        .text_px(13.)
}

fn field_row(field: &Entity<PromptInput>, size: f32, theme: &Theme) -> gpui::Div {
    div()
        .flex_1()
        .min_w_0()
        .text_px(size)
        .line_height(u(20.))
        .text_color(theme.colors.content)
        .child(field.clone())
}

fn dialog_button(
    id: &'static str,
    label: &'static str,
    primary: bool,
    disabled: bool,
    busy: bool,
    theme: &Theme,
) -> gpui::Stateful<gpui::Div> {
    let (bg, ink, hover) = if primary {
        (
            theme.colors.content,
            theme.colors.background_base,
            theme.content(0.80),
        )
    } else {
        (
            gpui::transparent_black(),
            theme.content(0.70),
            theme.content(0.08),
        )
    };
    let mut button = div()
        .id(id)
        .flex()
        .items_center()
        .gap(u(6.))
        .rounded(u(theme.radius.md))
        .px(u(12.))
        .py(u(6.))
        .text_px(12.)
        .bg(bg)
        .text_color(ink);
    if primary {
        button = button.medium();
    }
    if busy {
        button = button.child(
            spinner(SharedString::from(format!("{id}-spinner")))
                .size(14.)
                .color(ink),
        );
    }
    button = button.child(label);
    if disabled {
        button.opacity(0.4)
    } else {
        button.hover(move |style| style.bg(hover))
    }
}

fn error_text(message: &str, theme: &Theme) -> gpui::Div {
    div()
        .text_px(11.)
        .line_height(u(16.))
        .text_color(colors::red(theme, 0.9))
        .child(message.to_string())
}

impl QuickGitPopup {
    fn render_waiting(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let hover = theme.content(0.10);
        div()
            .flex()
            .items_center()
            .gap(u(12.))
            .px(u(12.))
            .py(u(12.))
            .text_px(12.)
            .child(
                div()
                    .flex_1()
                    .text_color(theme.content(0.60))
                    .child(if self.settled {
                        "Couldn\u{2019}t load branches for this project."
                    } else {
                        "Loading branches\u{2026}"
                    }),
            )
            .child(
                div()
                    .id("quick-git-close")
                    .rounded(u(theme.radius.md))
                    .px(u(8.))
                    .py(u(4.))
                    .text_color(theme.content(0.70))
                    .hover(move |style| style.bg(hover))
                    .on_click(cx.listener(|this, _, _, cx| this.finish(None, cx)))
                    .child("Close"),
            )
            .into_any_element()
    }

    fn search_row(
        &self,
        theme: &Theme,
        size: f32,
        px_x: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(u(8.))
            .border_b_1()
            .border_color(theme.colors.stroke)
            .px(u(px_x))
            .py(u(10.))
            .capture_action(cx.listener(|this, _: &prompt_input::MoveDown, _, cx| {
                cx.stop_propagation();
                this.step(true, cx);
            }))
            .capture_action(cx.listener(|this, _: &prompt_input::MoveUp, _, cx| {
                cx.stop_propagation();
                this.step(false, cx);
            }))
            .capture_action(cx.listener(|this, _: &prompt_input::Enter, window, cx| {
                cx.stop_propagation();
                if !this.search.read(cx).is_composing() {
                    this.enter(window, cx);
                }
            }))
            .capture_action(cx.listener(|_, _: &prompt_input::Newline, _, cx| {
                cx.stop_propagation();
            }))
            .child(
                icon(IconName::Search)
                    .size(u(14.))
                    .text_color(theme.content(0.50)),
            )
            .child(field_row(&self.search, size, theme))
            .into_any_element()
    }

    fn render_branch_picker(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let rows = self.branch_rows(cx);
        let query = self.search.read(cx).text().to_string();
        let mut list = div()
            .id("quick-git-branches")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(u(6.))
            .py(u(6.));
        if rows.is_empty() {
            list = list.child(
                div()
                    .px(u(12.))
                    .py(u(16.))
                    .text_px(12.)
                    .text_color(theme.content(0.50))
                    .child(if query.trim().is_empty() {
                        "No branches"
                    } else {
                        "No matching branches"
                    }),
            );
        }
        for (index, row) in rows.into_iter().enumerate() {
            let highlighted = index == self.active || row.current;
            let hover = theme.content(0.05);
            let mut item = row_base(
                SharedString::from(format!(
                    "quick-git-branch-{}-{}",
                    row.remote.as_deref().unwrap_or("local"),
                    row.name
                )),
                theme,
            )
            .h(u(32.))
            .text_color(theme.colors.content)
            .on_mouse_move(cx.listener(move |this, _, _, cx| {
                if this.active != index {
                    this.active = index;
                    cx.notify();
                }
            }))
            .on_click(cx.listener(move |this, _, window, cx| this.pick_branch(index, window, cx)));
            item = if highlighted {
                item.bg(theme.colors.selection)
            } else {
                item.hover(move |style| style.bg(hover))
            };
            if self.busy {
                item = item.opacity(0.6);
            }
            item = item.child(if row.current {
                icon(IconName::Check)
                    .size(u(14.))
                    .text_color(theme.colors.content)
            } else {
                icon(IconName::GitBranch)
                    .size(u(14.))
                    .text_color(theme.content(0.50))
            });
            let mut name = div().min_w_0().flex_1().truncate().child(row.name.clone());
            if row.current {
                name = name.medium();
            }
            item = item.child(name);
            if let Some(remote) = &row.remote {
                item = item.child(
                    div()
                        .flex_none()
                        .rounded(u(theme.radius.sm))
                        .bg(theme.content(0.06))
                        .px(u(6.))
                        .py(u(2.))
                        .text_px(10.)
                        .text_color(theme.content(0.40))
                        .child(remote.clone()),
                );
            }
            list = list.child(item);
        }
        let mut picker = div()
            .flex()
            .flex_col()
            .min_h(u(180.))
            .max_h(u(280.))
            .child(self.search_row(theme, 13., 12., cx))
            .child(list);
        if let Some(error) = &self.error {
            picker = picker.child(
                div()
                    .flex_none()
                    .border_t_1()
                    .border_color(theme.colors.stroke)
                    .px(u(10.))
                    .py(u(8.))
                    .child(error_text(error, theme)),
            );
        }
        if let Some(name) = create_row(self.branches.as_ref(), &query) {
            let count = self.branch_rows(cx).len();
            let hover = theme.content(0.08);
            let hover_ink = theme.colors.content;
            let mut button = row_base("quick-git-create".into(), theme)
                .h(u(30.))
                .gap(u(10.))
                .px(u(10.))
                .text_color(theme.content(0.75))
                .on_click(
                    cx.listener(move |this, _, window, cx| this.pick_branch(count, window, cx)),
                )
                .child(
                    icon(IconName::Plus)
                        .size(u(16.))
                        .text_color(theme.content(0.75)),
                )
                .child(div().min_w_0().truncate().child(create_label(&name)));
            button = if self.busy {
                button.opacity(0.6)
            } else {
                button.hover(move |style| style.bg(hover).text_color(hover_ink))
            };
            picker = picker.child(
                div()
                    .flex_none()
                    .border_t_1()
                    .border_color(theme.colors.stroke)
                    .p(u(4.))
                    .px(u(6.))
                    .child(button),
            );
        }
        picker.into_any_element()
    }

    fn render_create(
        &self,
        error: Option<&str>,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let name = self.name.read(cx).text().trim().to_string();
        let disabled = name.is_empty() || self.busy;
        let mut create = dialog_button(
            "quick-git-create-branch",
            "Create branch",
            true,
            disabled,
            self.busy,
            theme,
        );
        if !disabled {
            create =
                create.on_click(cx.listener(|this, _, window, cx| this.create_branch(window, cx)));
        }
        let mut cancel = dialog_button(
            "quick-git-create-cancel",
            "Cancel",
            false,
            self.busy,
            false,
            theme,
        );
        if !self.busy {
            cancel = cancel.on_click(cx.listener(|this, _, _, cx| this.cancel_dialog(cx)));
        }
        let mut form = div()
            .flex()
            .flex_col()
            .gap(u(16.))
            .p(u(16.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(u(4.))
                    .child(div().text_px(13.).medium().child("New branch"))
                    .child(
                        div()
                            .text_px(12.)
                            .text_color(theme.content(0.55))
                            .child("Create and check out a branch in this project."),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(u(6.))
                    .child(
                        div()
                            .text_px(12.)
                            .medium()
                            .text_color(theme.content(0.70))
                            .child("Branch name"),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .h(u(36.))
                            .rounded(u(theme.radius.md))
                            .border_1()
                            .border_color(theme.content(0.10))
                            .bg(theme.content(0.05))
                            .px(u(10.))
                            .capture_action(cx.listener(
                                |this, _: &prompt_input::Enter, window, cx| {
                                    cx.stop_propagation();
                                    this.create_branch(window, cx);
                                },
                            ))
                            .capture_action(cx.listener(|_, _: &prompt_input::Newline, _, cx| {
                                cx.stop_propagation();
                            }))
                            .child(field_row(&self.name, 13., theme)),
                    ),
            );
        if let Some(error) = error {
            form = form.child(error_text(error, theme));
        }
        form.child(
            div()
                .flex()
                .justify_end()
                .gap(u(8.))
                .child(cancel)
                .child(create),
        )
        .into_any_element()
    }

    fn render_blocked(
        &self,
        pending: &PendingSwitch,
        busy: Option<&'static str>,
        error: Option<&str>,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let message = self.message.read(cx).text().trim().to_string();
        let can_commit = !message.is_empty() && busy.is_none();
        let mut cancel = dialog_button(
            "quick-git-blocked-cancel",
            "Cancel",
            false,
            busy.is_some(),
            false,
            theme,
        );
        if busy.is_none() {
            cancel = cancel.on_click(cx.listener(|this, _, _, cx| this.cancel_dialog(cx)));
        }
        let commit_hover = theme.content(0.15);
        let mut commit = div()
            .id("quick-git-commit-switch")
            .flex()
            .items_center()
            .gap(u(6.))
            .rounded(u(theme.radius.md))
            .bg(theme.content(0.10))
            .px(u(12.))
            .py(u(6.))
            .text_px(12.)
            .medium()
            .text_color(theme.colors.content);
        if busy == Some("commit") {
            commit = commit.child(
                spinner("quick-git-commit-spinner")
                    .size(14.)
                    .color(theme.colors.content),
            );
        }
        commit = commit.child("Commit & switch");
        commit = if can_commit {
            commit
                .hover(move |style| style.bg(commit_hover))
                .on_click(cx.listener(|this, _, window, cx| this.resolve_blocked(true, window, cx)))
        } else {
            commit.opacity(0.4)
        };
        let mut stash = dialog_button(
            "quick-git-stash-switch",
            "Stash & switch",
            true,
            busy.is_some(),
            busy == Some("stash"),
            theme,
        );
        if busy.is_none() {
            stash = stash.on_click(
                cx.listener(|this, _, window, cx| this.resolve_blocked(false, window, cx)),
            );
        }
        let mut panel = div()
            .flex()
            .flex_col()
            .gap(u(12.))
            .m(u(8.))
            .rounded(u(theme.radius.lg))
            .border_1()
            .border_color(theme.content(0.10))
            .bg(theme.content(0.05))
            .p(u(16.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(u(4.))
                    .child(div().text_px(13.).medium().child("Uncommitted changes"))
                    .child(
                        div()
                            .text_px(12.)
                            .text_color(theme.content(0.55))
                            .child(blocked_message(pending)),
                    ),
            )
            .child(
                div()
                    .rounded(u(theme.radius.md))
                    .bg(theme.content(0.10))
                    .px(u(8.))
                    .py(u(4.))
                    .capture_action(cx.listener(|this, _: &prompt_input::Enter, window, cx| {
                        let modifiers = window.modifiers();
                        if modifiers.platform || modifiers.control {
                            cx.stop_propagation();
                            this.resolve_blocked(true, window, cx);
                        }
                    }))
                    .child(field_row(&self.message, 13., theme)),
            );
        if let Some(error) = error {
            panel = panel.child(error_text(error, theme));
        }
        panel
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .justify_end()
                    .gap(u(8.))
                    .child(cancel)
                    .child(commit)
                    .child(stash),
            )
            .into_any_element()
    }

    fn render_running(
        &self,
        pending: &PendingSwitch,
        message: &str,
        busy: bool,
        error: Option<&str>,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let title = if pending.creating() {
            format!("Create {} anyway?", pending.name())
        } else {
            format!("Switch to {} anyway?", pending.name())
        };
        let mut cancel = dialog_button(
            "quick-git-running-cancel",
            "Cancel",
            false,
            busy,
            false,
            theme,
        );
        if !busy {
            cancel = cancel.on_click(cx.listener(|this, _, _, cx| this.cancel_dialog(cx)));
        }
        let red_hover = colors::red(theme, 0.3);
        let mut confirm = div()
            .id("quick-git-running-confirm")
            .rounded(u(theme.radius.md))
            .bg(colors::red(theme, 0.2))
            .px(u(12.))
            .py(u(6.))
            .text_px(12.)
            .medium()
            .text_color(colors::red(theme, 1.0))
            .child(if pending.creating() {
                "Create and switch"
            } else {
                "Switch anyway"
            });
        confirm = if busy {
            confirm.opacity(0.5)
        } else {
            confirm
                .hover(move |style| style.bg(red_hover))
                .on_click(cx.listener(|this, _, window, cx| this.confirm_running(window, cx)))
        };
        let mut body = div()
            .flex()
            .flex_col()
            .gap(u(16.))
            .p(u(16.))
            .text_px(12.)
            .child(div().text_px(13.).medium().child(title))
            .child(div().child(running_sessions_message(message)));
        if let Some(error) = error {
            body = body.child(error_text(error, theme));
        }
        body.child(
            div()
                .flex()
                .justify_end()
                .gap(u(8.))
                .child(cancel)
                .child(confirm),
        )
        .into_any_element()
    }

    fn render_mode_picker(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let mode = self
            .choice()
            .map(|choice| choice.mode)
            .unwrap_or(WorkspaceMode::Current);
        let mut menu = div().flex().flex_col().p(u(6.)).child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(u(12.))
                .px(u(8.))
                .py(u(4.))
                .text_px(11.)
                .medium()
                .text_color(theme.content(0.45))
                .child("Workspace")
                .child(
                    div()
                        .text_px(10.)
                        .text_color(theme.content(0.35))
                        .child("\u{2318}\u{21e7}G"),
                ),
        );
        for (value, label, glyph) in [
            (WorkspaceMode::Current, "Current checkout", IconName::Folder),
            (
                WorkspaceMode::Worktree,
                "New worktree",
                IconName::FolderTree,
            ),
        ] {
            let selected = mode == value;
            let hover = theme.content(0.08);
            let mut row = row_base(SharedString::from(format!("quick-git-mode-{label}")), theme)
                .h(u(36.))
                .text_color(if selected {
                    theme.colors.content
                } else {
                    theme.content(0.80)
                })
                .hover(move |style| style.bg(hover))
                .on_mouse_move(cx.listener(|this, _, _, cx| this.close_worktree_menu(cx)))
                .on_click(cx.listener(move |this, _, _, cx| this.choose_mode(value, cx)))
                .child(icon(glyph).size(u(16.)).text_color(theme.content(0.55)))
                .child(div().flex_1().child(label));
            if selected {
                row = row.bg(theme.colors.selection).child(
                    icon(IconName::Check)
                        .size(u(14.))
                        .text_color(theme.colors.content),
                );
            }
            menu = menu.child(row);
        }
        let hover = theme.content(0.08);
        let hover_ink = theme.colors.content;
        let mut existing = row_base("quick-git-existing".into(), theme)
            .h(u(36.))
            .text_color(theme.content(0.80))
            .hover(move |style| style.bg(hover).text_color(hover_ink))
            .on_mouse_move(cx.listener(|this, _, _, cx| this.open_worktree_menu(cx)))
            .on_click(cx.listener(|this, _, _, cx| this.open_worktree_menu(cx)))
            .child(
                icon(IconName::FolderTree)
                    .size(u(16.))
                    .text_color(theme.content(0.55)),
            )
            .child(div().flex_1().child("Existing worktree\u{2026}"))
            .child(
                icon(IconName::ChevronRight)
                    .size(u(14.))
                    .text_color(theme.content(0.45)),
            );
        if self.worktrees.open {
            existing = existing
                .bg(theme.colors.selection)
                .text_color(theme.colors.content);
        }
        menu = menu.child(existing);
        if self.worktrees.open {
            menu = menu.child(self.render_worktrees(theme, cx));
        }
        menu.into_any_element()
    }

    fn render_worktrees(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let mut submenu = div()
            .flex()
            .flex_col()
            .mt(u(4.))
            .border_t_1()
            .border_color(theme.colors.stroke)
            .pt(u(4.))
            .max_h(u(320.));
        match &self.worktrees.data {
            None => {
                submenu = submenu.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(u(8.))
                        .px(u(8.))
                        .py(u(12.))
                        .text_px(12.)
                        .text_color(theme.content(0.50))
                        .child(
                            spinner("quick-git-worktrees-loading")
                                .size(14.)
                                .color(theme.content(0.50)),
                        )
                        .child("Loading worktrees\u{2026}"),
                );
            }
            Some(Ok(listed)) => {
                let trees = existing_worktrees(listed);
                if trees.is_empty() {
                    submenu = submenu.child(
                        div()
                            .px(u(8.))
                            .py(u(12.))
                            .text_px(12.)
                            .text_color(theme.content(0.50))
                            .child("No existing worktrees"),
                    );
                }
                let mut list = div()
                    .id("quick-git-worktree-list")
                    .flex()
                    .flex_col()
                    .min_h_0()
                    .overflow_y_scroll();
                for tree in trees {
                    let busy = self.worktrees.busy_path.as_deref() == Some(tree.path.as_str());
                    let hover = theme.content(0.08);
                    let hover_ink = theme.colors.content;
                    let glyph: AnyElement = if busy {
                        spinner(SharedString::from(format!(
                            "quick-git-tree-busy-{}",
                            tree.path
                        )))
                        .size(16.)
                        .color(theme.content(0.55))
                        .into_any_element()
                    } else {
                        icon(IconName::FolderTree)
                            .size(u(16.))
                            .text_color(theme.content(0.55))
                            .into_any_element()
                    };
                    let picked = tree.clone();
                    let mut row = div()
                        .id(SharedString::from(format!("quick-git-tree-{}", tree.path)))
                        .flex()
                        .flex_none()
                        .min_h(u(44.))
                        .w_full()
                        .items_center()
                        .gap(u(8.))
                        .rounded(u(theme.radius.lg))
                        .px(u(8.))
                        .py(u(6.))
                        .text_color(theme.content(0.80))
                        .tooltip(monocode_ui::widgets::tooltip(SharedString::from(
                            tree.path.clone(),
                        )))
                        .child(glyph)
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .min_w_0()
                                .flex_1()
                                .child(div().text_px(13.).truncate().child(worktree_title(&tree)))
                                .child(
                                    div()
                                        .truncate()
                                        .font_family(theme.fonts.mono.clone())
                                        .text_px(10.)
                                        .text_color(theme.content(0.40))
                                        .child(pretty_cwd(&tree.path)),
                                ),
                        );
                    row = if self.worktrees.busy_path.is_some() {
                        row.opacity(0.4)
                    } else {
                        row.hover(move |style| style.bg(hover).text_color(hover_ink))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.select_worktree(picked.clone(), cx)
                            }))
                    };
                    list = list.child(row);
                }
                submenu = submenu.child(list);
            }
            Some(Err(error)) => {
                submenu = submenu.child(
                    div()
                        .px(u(8.))
                        .py(u(8.))
                        .text_px(11.)
                        .text_color(colors::red(theme, 1.0))
                        .child(error.clone()),
                );
            }
        }
        submenu.into_any_element()
    }

    fn render_base_picker(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let rows = self.base_rows(cx);
        let selected = self.effective_base();
        let mut list = div()
            .id("quick-git-bases")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p(u(6.));
        for (index, row) in rows.iter().enumerate() {
            let reference = row.reference();
            let is_selected = reference == selected;
            let highlighted = index == self.active || is_selected;
            let hover = theme.content(0.08);
            let mut item = row_base(
                SharedString::from(format!("quick-git-base-{reference}")),
                theme,
            )
            .h(u(32.))
            .text_color(if highlighted {
                theme.colors.content
            } else {
                theme.content(0.80)
            })
            .hover(move |style| style.bg(hover))
            .on_mouse_move(cx.listener(move |this, _, _, cx| {
                if this.active != index {
                    this.active = index;
                    cx.notify();
                }
            }))
            .on_click(cx.listener(move |this, _, _, cx| this.choose_base(index, cx)))
            .child(if is_selected {
                icon(IconName::Check)
                    .size(u(14.))
                    .text_color(theme.colors.content)
            } else {
                icon(IconName::GitBranch)
                    .size(u(14.))
                    .text_color(theme.content(0.45))
            });
            if highlighted {
                item = item.bg(theme.colors.selection);
            }
            let mut name = div().min_w_0().flex_1().truncate().child(reference.clone());
            if is_selected {
                name = name.medium();
            }
            list = list.child(item.child(name));
        }
        if rows.is_empty() {
            list = list.child(
                div()
                    .px(u(8.))
                    .py(u(12.))
                    .text_px(12.)
                    .text_color(theme.content(0.45))
                    .child("No matching branches"),
            );
        }
        div()
            .flex()
            .flex_col()
            .min_h(u(160.))
            .max_h(u(280.))
            .child(self.search_row(theme, 12., 8., cx))
            .child(list)
            .into_any_element()
    }
}

/// The popup's panel width, in CSS px.
pub const GIT_POPUP_WIDTH: f32 = 320.0;

impl Render for QuickGitPopup {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let content: AnyElement = match (&self.request, self.waiting()) {
            (None, _) => div().into_any_element(),
            (Some(_), true) => self.render_waiting(&theme, cx),
            (Some(request), false) => match &self.stage {
                BranchStage::Create { error } => {
                    let error = error.clone();
                    self.render_create(error.as_deref(), &theme, cx)
                }
                BranchStage::Blocked {
                    pending,
                    busy,
                    error,
                } => {
                    let (pending, busy, error) = (pending.clone(), *busy, error.clone());
                    self.render_blocked(&pending, busy, error.as_deref(), &theme, cx)
                }
                BranchStage::Running {
                    pending,
                    message,
                    busy,
                    error,
                } => {
                    let (pending, message, busy, error) =
                        (pending.clone(), message.clone(), *busy, error.clone());
                    self.render_running(&pending, &message, busy, error.as_deref(), &theme, cx)
                }
                BranchStage::List => match request.kind {
                    QuickGitKind::Branch => self.render_branch_picker(&theme, cx),
                    QuickGitKind::Workspace => self.render_mode_picker(&theme, cx),
                    QuickGitKind::Base => self.render_base_picker(&theme, cx),
                },
            },
        };
        let id = self.request.as_ref().map(|request| request.id.clone());
        let entity = cx.entity().downgrade();
        let probe = canvas(
            move |bounds, _, cx| {
                let (Some(id), Some(entity)) = (id, entity.upgrade()) else {
                    return;
                };
                let height = f32::from(bounds.size.height).ceil().max(0.) as u32;
                entity.update(cx, |this, cx| {
                    if height > 2 && this.fit_sent != Some(height) {
                        this.fit_sent = Some(height);
                        cx.emit(QuickGitPopupEvent::Fit { id, height });
                    }
                });
            },
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full();
        let mut body = div()
            .relative()
            .flex()
            .flex_col()
            .flex_none()
            .w_full()
            .child(probe);
        if let Some(error) = &self.finish_error {
            body = body.child(
                div()
                    .px(u(12.))
                    .py(u(8.))
                    .text_px(12.)
                    .text_color(colors::red(&theme, 1.0))
                    .child(error.clone()),
            );
        }
        body = body.child(content);
        div()
            .id("quick-git-popup")
            .key_context("QuickGitPopup")
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::on_key_down))
            .w_full()
            .max_h(u(520.))
            .overflow_y_scroll()
            .rounded(u(theme.radius.xl))
            .border_1()
            .border_color(theme.content(0.10))
            .bg(theme.colors.background_base.opacity(0.45))
            .text_color(theme.colors.content)
            .font_family(theme.fonts.sans.clone())
            .line_height(gpui::relative(theme.leading.normal))
            .child(body)
    }
}

#[allow(dead_code)]
fn _unused(_: Duration, _: Hsla, _: Subscription) {}

#[allow(dead_code)]
fn _kbd() -> impl IntoElement {
    kbd("")
}
