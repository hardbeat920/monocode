//! Port of src/features/workspace/ui/WorkspacePicker.tsx: the composer's
//! workspace mode toggle (current checkout or a new worktree, ⌘⇧G), the
//! existing worktree submenu, and the worktree base branch picker.
//!
//! The React picker loaded branches and worktrees itself. Here the owner
//! passes the branches in and answers [`WorkspacePickerEvent::LoadWorktrees`]
//! with [`WorkspacePicker::set_worktrees`]; choosing a worktree reports
//! [`WorkspacePickerEvent::SelectWorktree`] and the owner replies with
//! [`WorkspacePicker::worktree_selected`].

use std::rc::Rc;
use std::time::Duration;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    Anchor, AnyElement, App, AppContext as _, Bounds, Context, ElementId, Entity, EventEmitter,
    FocusHandle, Focusable as _, InteractiveElement as _, IntoElement, KeyBinding, KeyDownEvent,
    Keystroke, MouseButton, MouseDownEvent, ParentElement as _, Pixels, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Task, Window, actions, canvas, div,
    point, px,
};
use gpui_base::input::InputEditorStyle;
use gpui_component::input::{InputEvent, InputState};
use monocode_core::session::WorkspaceMode;
use monocode_layout::paths::pretty_cwd;
use monocode_ui::widgets::{POPOVER_GAP, PopoverSide, popover_at, popover_frame, spinner, tooltip};
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

use super::submenu::submenu_beside;

actions!(workspace_picker, [ToggleWorkspaceMode]);

/// `WORKSPACE_MODE_SHORTCUT`.
pub const WORKSPACE_MODE_SHORTCUT: &str = if cfg!(target_os = "macos") {
    "⌘⇧G"
} else {
    "Ctrl+Shift+G"
};
/// How long the worktree submenu waits before closing on pointer leave.
const HOVER_CLOSE_MS: u64 = 100;
/// `SUBMENU_GAP`.
const SUBMENU_GAP: f32 = 4.0;

/// Binds ⌘⇧G (Ctrl+Shift+G) to [`ToggleWorkspaceMode`]. The composer that
/// holds the picker handles the action and calls
/// [`WorkspacePicker::toggle_mode`], because the key reaches the focused
/// prompt, not the picker.
pub fn init(cx: &mut App) {
    let key = if cfg!(target_os = "macos") {
        "cmd-shift-g"
    } else {
        "ctrl-shift-g"
    };
    cx.bind_keys([KeyBinding::new(key, ToggleWorkspaceMode, None)]);
}

/// `isWorkspaceModeShortcut` without a keybinding override: Cmd or Ctrl,
/// Shift, no Alt, and G.
pub fn is_workspace_mode_shortcut(keystroke: &Keystroke) -> bool {
    let modifiers = keystroke.modifiers;
    (modifiers.platform || modifiers.control)
        && modifiers.shift
        && !modifiers.alt
        && keystroke.key.eq_ignore_ascii_case("g")
}

/// What the composer's shortcut does: flip between the current checkout
/// and a new worktree. A worktree needs a base, so without one nothing
/// happens.
pub fn toggle_workspace_mode(
    mode: WorkspaceMode,
    resolved_base: Option<&str>,
) -> Option<(WorkspaceMode, Option<String>)> {
    match mode {
        WorkspaceMode::Current => {
            resolved_base.map(|base| (WorkspaceMode::Worktree, Some(base.to_string())))
        }
        WorkspaceMode::Worktree => Some((WorkspaceMode::Current, None)),
    }
}

/// `BaseBranch`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaseBranch {
    pub name: String,
    pub remote: Option<String>,
}

/// `branchRef`.
pub fn branch_ref(branch: &BaseBranch) -> String {
    match &branch.remote {
        Some(remote) => format!("{remote}/{}", branch.name),
        None => branch.name.clone(),
    }
}

/// The base picker's rows: unique by ref (a later duplicate replaces the
/// earlier one in place, as a `Map` did), filtered by `query`.
pub fn base_branch_rows(branches: &[BaseBranch], query: &str) -> Vec<BaseBranch> {
    let needle = query.trim().to_lowercase();
    let mut unique: Vec<(String, BaseBranch)> = Vec::new();
    for branch in branches {
        let key = branch_ref(branch);
        match unique.iter_mut().find(|(existing, _)| *existing == key) {
            Some(entry) => entry.1 = branch.clone(),
            None => unique.push((key, branch.clone())),
        }
    }
    unique
        .into_iter()
        .filter(|(key, _)| key.to_lowercase().contains(&needle))
        .map(|(_, branch)| branch)
        .collect()
}

/// A project's branches, as `useProjectBranchesState` returned them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectBranches {
    pub current: Option<String>,
    pub branches: Vec<BaseBranch>,
}

/// `Worktree`, the fields the picker reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeEntry {
    pub path: String,
    pub branch: Option<String>,
    pub head: String,
    pub is_main: bool,
    pub missing: bool,
}

/// Which popover opens at mount (`initialPicker`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitialPicker {
    Workspace,
    Base,
}

/// What the picker shows.
#[derive(Debug, Clone)]
pub struct WorkspacePickerProps {
    pub cwd: String,
    pub mode: WorkspaceMode,
    pub base: Option<String>,
    pub enabled: bool,
    /// `None` while the branches load.
    pub branches: Option<ProjectBranches>,
    /// The branch load finished.
    pub settled: bool,
    /// `onSelectWorktree` is set: the existing worktree row shows.
    pub can_select_worktree: bool,
    /// `onOpenSettings` is set: the settings row shows.
    pub can_open_settings: bool,
    pub popover_side: PopoverSide,
    /// The shortcut label, or `None` when the user disabled it.
    pub shortcut: Option<SharedString>,
}

impl Default for WorkspacePickerProps {
    fn default() -> Self {
        Self {
            cwd: String::new(),
            mode: WorkspaceMode::Current,
            base: None,
            enabled: true,
            branches: None,
            settled: false,
            can_select_worktree: false,
            can_open_settings: false,
            popover_side: PopoverSide::Top,
            shortcut: Some(WORKSPACE_MODE_SHORTCUT.into()),
        }
    }
}

impl WorkspacePickerProps {
    /// `resolvedBase`: the chosen base, else the current branch.
    pub fn resolved_base(&self) -> Option<String> {
        self.base
            .clone()
            .filter(|base| !base.is_empty())
            .or_else(|| {
                self.branches
                    .as_ref()
                    .and_then(|branches| branches.current.clone())
            })
            .filter(|base| !base.is_empty())
    }

    /// `effectiveBase`.
    pub fn effective_base(&self) -> String {
        self.resolved_base().unwrap_or_else(|| "HEAD".into())
    }

    fn mode_enabled(&self) -> bool {
        self.enabled && self.resolved_base().is_some()
    }

    fn base_enabled(&self) -> bool {
        self.enabled && self.branches.is_some()
    }
}

/// What the picker reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspacePickerEvent {
    /// `onModeChange`: a worktree carries its base.
    ModeChange {
        mode: WorkspaceMode,
        base: Option<String>,
    },
    /// `onBaseChange`.
    BaseChange(String),
    /// `onSelectWorktree`. Reply with [`WorkspacePicker::worktree_selected`].
    SelectWorktree(WorktreeEntry),
    /// `onOpenSettings`.
    OpenSettings,
    /// `onClose`: a popover closed; the composer takes focus back.
    Close,
    /// `onOpenChange`.
    OpenChange(bool),
    /// The worktree submenu opened: reply with
    /// [`WorkspacePicker::set_worktrees`].
    LoadWorktrees { cwd: String },
}

/// The workspace mode toggle and, in worktree mode, the base picker.
pub struct WorkspacePicker {
    props: WorkspacePickerProps,
    mode_open: bool,
    base_open: bool,
    worktree_menu: bool,
    worktrees: Option<Result<Vec<WorktreeEntry>, String>>,
    busy_path: Option<String>,
    pick_error: Option<String>,
    close_worktree_timer: Option<Task<()>>,
    query: Entity<InputState>,
    active: usize,
    focus: FocusHandle,
    mode_trigger: Rc<std::cell::Cell<Bounds<Pixels>>>,
    base_trigger: Rc<std::cell::Cell<Bounds<Pixels>>>,
    worktree_row: Rc<std::cell::Cell<Bounds<Pixels>>>,
    /// The pointer is over the worktree submenu.
    submenu_hovered: bool,
    animate: bool,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<WorkspacePickerEvent> for WorkspacePicker {}

impl WorkspacePicker {
    pub fn new(props: WorkspacePickerProps, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let query = cx.new(|cx| InputState::new(window, cx).placeholder("Search base branches…"));
        let subscriptions =
            vec![
                cx.subscribe_in(&query, window, |this, _, event: &InputEvent, window, cx| {
                    match event {
                        InputEvent::Change => {
                            this.active = 0;
                            cx.notify();
                        }
                        InputEvent::PressEnter { .. } => this.pick_active_base(window, cx),
                        _ => {}
                    }
                }),
            ];
        Self {
            props,
            mode_open: false,
            base_open: false,
            worktree_menu: false,
            worktrees: None,
            busy_path: None,
            pick_error: None,
            close_worktree_timer: None,
            query,
            active: 0,
            focus: cx.focus_handle(),
            mode_trigger: Rc::default(),
            base_trigger: Rc::default(),
            worktree_row: Rc::default(),
            submenu_hovered: false,
            animate: true,
            _subscriptions: subscriptions,
        }
    }

    pub fn props(&self) -> &WorkspacePickerProps {
        &self.props
    }

    /// New props. Disabling the picker closes its popovers.
    pub fn set_props(
        &mut self,
        props: WorkspacePickerProps,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.props = props;
        if !self.props.mode_enabled() && self.mode_open {
            self.mode_open = false;
            self.worktree_menu = false;
            self.busy_path = None;
            self.pick_error = None;
            self.report_open(cx);
        }
        if !self.props.base_enabled() && self.base_open {
            self.base_open = false;
            self.report_open(cx);
        }
        let rows = self.rows(cx).len();
        self.active = self.active.min(rows.saturating_sub(1));
        let _ = window;
        cx.notify();
    }

    /// Turns the popover animation off, for screenshots.
    pub fn set_animate(&mut self, animate: bool) {
        self.animate = animate;
    }

    pub fn is_mode_open(&self) -> bool {
        self.mode_open
    }

    pub fn is_base_open(&self) -> bool {
        self.base_open
    }

    /// Opens the existing worktree submenu, as hovering its row does.
    pub fn show_worktree_menu(&mut self, cx: &mut Context<Self>) {
        if self.mode_open {
            self.open_worktree_menu(cx);
        }
    }

    pub fn is_worktree_menu_open(&self) -> bool {
        self.worktree_menu
    }

    /// Opens a popover, as `initialPicker` did at mount.
    pub fn open(&mut self, picker: InitialPicker, window: &mut Window, cx: &mut Context<Self>) {
        match picker {
            InitialPicker::Workspace => {
                if self.props.mode_enabled() {
                    self.mode_open = true;
                    window.focus(&self.focus, cx);
                }
            }
            InitialPicker::Base => {
                if self.props.mode == WorkspaceMode::Worktree && self.props.base_enabled() {
                    self.open_base(window, cx);
                }
            }
        }
        self.report_open(cx);
        cx.notify();
    }

    /// The ⌘⇧G toggle (the composer's `onComposerKeyDown`).
    pub fn toggle_mode(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.props.enabled {
            return false;
        }
        let base = self.props.resolved_base();
        let Some((mode, base)) = toggle_workspace_mode(self.props.mode, base.as_deref()) else {
            return false;
        };
        cx.emit(WorkspacePickerEvent::ModeChange { mode, base });
        true
    }

    /// The worktree list the owner loaded, or why it failed.
    pub fn set_worktrees(
        &mut self,
        worktrees: Result<Vec<WorktreeEntry>, String>,
        cx: &mut Context<Self>,
    ) {
        self.worktrees = Some(worktrees);
        cx.notify();
    }

    /// The owner finished switching to a worktree: success closes the
    /// popover, an error shows under the list.
    pub fn worktree_selected(
        &mut self,
        result: Result<(), String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(()) => self.dismiss_mode(window, cx),
            Err(error) => {
                self.pick_error = Some(error);
                self.busy_path = None;
                cx.notify();
            }
        }
    }

    fn report_open(&mut self, cx: &mut Context<Self>) {
        cx.emit(WorkspacePickerEvent::OpenChange(
            self.mode_open || self.base_open,
        ));
    }

    fn label(&self) -> &'static str {
        if self.props.mode == WorkspaceMode::Worktree {
            "New worktree"
        } else {
            "Current checkout"
        }
    }

    /// The listed worktrees: not the main checkout, not missing.
    pub fn listed_worktrees(&self) -> Vec<WorktreeEntry> {
        match &self.worktrees {
            Some(Ok(trees)) => trees
                .iter()
                .filter(|tree| !tree.is_main && !tree.missing)
                .cloned()
                .collect(),
            _ => Vec::new(),
        }
    }

    fn rows(&self, cx: &App) -> Vec<BaseBranch> {
        let branches = self
            .props
            .branches
            .as_ref()
            .map(|b| b.branches.as_slice())
            .unwrap_or(&[]);
        base_branch_rows(branches, &self.query.read(cx).value())
    }

    /// `dismiss` for the mode popover.
    fn dismiss_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_worktree_timer = None;
        self.mode_open = false;
        self.worktree_menu = false;
        self.busy_path = None;
        self.pick_error = None;
        self.report_open(cx);
        self.release_focus(window, cx);
        cx.emit(WorkspacePickerEvent::Close);
        cx.notify();
    }

    fn release_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.focus.is_focused(window) || self.query.read(cx).focus_handle(cx).is_focused(window)
        {
            window.blur();
        }
    }

    fn toggle_mode_popover(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.mode_open {
            self.dismiss_mode(window, cx);
            return;
        }
        self.mode_open = true;
        window.focus(&self.focus, cx);
        self.report_open(cx);
        cx.notify();
    }

    fn change_mode(&mut self, mode: WorkspaceMode, window: &mut Window, cx: &mut Context<Self>) {
        let base = (mode == WorkspaceMode::Worktree).then(|| self.props.effective_base());
        cx.emit(WorkspacePickerEvent::ModeChange { mode, base });
        self.dismiss_mode(window, cx);
    }

    fn open_worktree_menu(&mut self, cx: &mut Context<Self>) {
        self.close_worktree_timer = None;
        if !self.worktree_menu {
            self.worktree_menu = true;
            if self.props.enabled && self.mode_open && self.props.can_select_worktree {
                cx.emit(WorkspacePickerEvent::LoadWorktrees {
                    cwd: self.props.cwd.clone(),
                });
            }
            cx.notify();
        }
    }

    fn close_worktree_menu(&mut self, cx: &mut Context<Self>) {
        self.close_worktree_timer = None;
        self.submenu_hovered = false;
        if self.worktree_menu || self.pick_error.is_some() {
            self.worktree_menu = false;
            self.pick_error = None;
            cx.notify();
        }
    }

    fn schedule_close_worktree_menu(&mut self, cx: &mut Context<Self>) {
        self.close_worktree_timer = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(HOVER_CLOSE_MS))
                .await;
            this.update(cx, |this, cx| {
                this.close_worktree_timer = None;
                this.worktree_menu = false;
                this.pick_error = None;
                cx.notify();
            })
            .ok();
        }));
    }

    fn select_worktree(&mut self, tree: WorktreeEntry, cx: &mut Context<Self>) {
        if !self.props.can_select_worktree || self.busy_path.is_some() {
            return;
        }
        self.busy_path = Some(tree.path.clone());
        self.pick_error = None;
        cx.emit(WorkspacePickerEvent::SelectWorktree(tree));
        cx.notify();
    }

    fn open_base(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.base_open = true;
        self.query.update(cx, |state, cx| {
            state.set_value("", window, cx);
            state.focus(window, cx);
        });
        self.active = 0;
    }

    fn toggle_base(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.base_open {
            self.dismiss_base(window, cx);
            return;
        }
        self.open_base(window, cx);
        self.report_open(cx);
        cx.notify();
    }

    fn dismiss_base(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.base_open = false;
        self.query
            .update(cx, |state, cx| state.set_value("", window, cx));
        self.report_open(cx);
        self.release_focus(window, cx);
        cx.emit(WorkspacePickerEvent::Close);
        cx.notify();
    }

    fn pick_base(&mut self, base: String, window: &mut Window, cx: &mut Context<Self>) {
        cx.emit(WorkspacePickerEvent::BaseChange(base));
        self.dismiss_base(window, cx);
    }

    fn pick_active_base(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(branch) = self.rows(cx).get(self.active) {
            self.pick_base(branch_ref(branch), window, cx);
        }
    }

    fn move_active(&mut self, delta: isize, cx: &mut Context<Self>) {
        let rows = self.rows(cx).len();
        if rows == 0 {
            return;
        }
        self.active = (self.active as isize + delta).clamp(0, rows as isize - 1) as usize;
        cx.notify();
    }

    fn trigger_style(theme: &Theme, enabled: bool, expanded: bool, max_width: f32) -> gpui::Div {
        let hover_bg = theme.content(0.08);
        let ink = theme.colors.content;
        let mut button = div()
            .flex()
            .h(u(24.))
            .min_w_0()
            .max_w(u(max_width))
            .ml(u(-6.))
            .items_center()
            .gap(u(6.))
            .rounded(u(theme.radius.md))
            .px(u(6.))
            .text_px(12.)
            .text_color(theme.content(0.55));
        if expanded {
            button = button.bg(hover_bg).text_color(ink);
        }
        if enabled {
            button.hover(move |s| s.bg(hover_bg).text_color(ink))
        } else {
            button.opacity(0.4)
        }
    }

    fn place(&self, trigger: Bounds<Pixels>, content: impl IntoElement, cx: &App) -> AnyElement {
        let gap = px(POPOVER_GAP);
        let (position, anchor) = match self.props.popover_side {
            PopoverSide::Bottom => (
                point(
                    trigger.origin.x,
                    trigger.origin.y + trigger.size.height + gap,
                ),
                Anchor::TopLeft,
            ),
            _ => (
                point(trigger.origin.x, trigger.origin.y - gap),
                Anchor::BottomLeft,
            ),
        };
        popover_at(position, anchor, content, cx).into_any_element()
    }

    fn render_mode_trigger(
        &self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let enabled = self.props.mode_enabled();
        let label = self.label();
        let icon_name = if self.props.mode == WorkspaceMode::Worktree {
            IconName::FolderTree
        } else {
            IconName::Folder
        };
        let bounds = self.mode_trigger.clone();
        let mut trigger = Self::trigger_style(theme, enabled, self.mode_open, 192.)
            .id("workspace-mode-trigger")
            .group("workspace-mode-trigger")
            .debug_selector(|| "workspace-mode-trigger".into())
            .relative()
            .child(trigger_icon(
                "workspace-mode-trigger",
                icon_name,
                self.mode_open,
                theme,
            ))
            .child(div().truncate().child(label))
            .child(
                canvas(move |rect, _, _| bounds.set(rect), |_, _, _, _| {})
                    .absolute()
                    .size_full(),
            )
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default());
        if let Some(shortcut) = self.props.shortcut.clone() {
            trigger = trigger.tooltip(tooltip(format!("Workspace: {label} ({shortcut})")));
        }
        if enabled {
            trigger = trigger
                .on_click(cx.listener(|this, _, window, cx| this.toggle_mode_popover(window, cx)));
        }
        trigger
    }

    fn render_mode_popover(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let mut body = div().flex().flex_col().p(u(6.)).child(
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
                .when_some(self.props.shortcut.clone(), |row, shortcut| {
                    row.child(
                        div()
                            .text_px(10.)
                            .text_color(theme.content(0.35))
                            .child(shortcut),
                    )
                }),
        );
        for (value, text, row_icon) in [
            (WorkspaceMode::Current, "Current checkout", IconName::Folder),
            (
                WorkspaceMode::Worktree,
                "New worktree",
                IconName::FolderTree,
            ),
        ] {
            let selected = self.props.mode == value;
            let hover = theme.content(0.08);
            body = body.child(
                div()
                    .id(ElementId::Name(format!("workspace-mode:{value:?}").into()))
                    .debug_selector(move || format!("workspace-mode:{value:?}"))
                    .flex()
                    .h(u(36.))
                    .w_full()
                    .items_center()
                    .gap(u(8.))
                    .rounded(u(theme.radius.lg))
                    .px(u(8.))
                    .text_px(13.)
                    .text_color(if selected {
                        theme.colors.content
                    } else {
                        theme.content(0.80)
                    })
                    .when(selected, |row| row.bg(theme.colors.selection))
                    .hover(move |s| s.bg(hover))
                    .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                        if *hovered {
                            this.close_worktree_menu(cx);
                        }
                    }))
                    .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.change_mode(value, window, cx)),
                    )
                    .child(
                        icon(row_icon)
                            .flex_none()
                            .size(u(16.))
                            .text_color(theme.content(0.55)),
                    )
                    .child(div().flex_1().child(text))
                    .when(selected, |row| {
                        row.child(
                            icon(IconName::Check)
                                .flex_none()
                                .size(u(14.))
                                .text_color(theme.colors.content),
                        )
                    }),
            );
        }
        if self.props.can_select_worktree {
            let hover = theme.content(0.08);
            let ink = theme.colors.content;
            let bounds = self.worktree_row.clone();
            body = body.child(
                div()
                    .id("workspace-existing-worktree")
                    .debug_selector(|| "workspace-existing-worktree".into())
                    .relative()
                    .flex()
                    .h(u(36.))
                    .w_full()
                    .items_center()
                    .gap(u(8.))
                    .rounded(u(theme.radius.lg))
                    .px(u(8.))
                    .text_px(13.)
                    .text_color(theme.content(0.80))
                    .when(self.worktree_menu, |row| {
                        row.bg(theme.colors.selection).text_color(ink)
                    })
                    .hover(move |s| s.bg(hover).text_color(ink))
                    .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                        if *hovered {
                            this.open_worktree_menu(cx);
                        } else {
                            this.schedule_close_worktree_menu(cx);
                        }
                    }))
                    .on_click(cx.listener(|this, _, _, cx| this.open_worktree_menu(cx)))
                    .child(
                        icon(IconName::FolderTree)
                            .flex_none()
                            .size(u(16.))
                            .text_color(theme.content(0.55)),
                    )
                    .child(div().flex_1().child("Existing worktree…"))
                    .child(
                        icon(IconName::ChevronRight)
                            .flex_none()
                            .size(u(14.))
                            .text_color(theme.content(0.45)),
                    )
                    .child(
                        canvas(move |rect, _, _| bounds.set(rect), |_, _, _, _| {})
                            .absolute()
                            .size_full(),
                    ),
            );
        }
        if self.props.can_open_settings {
            let hover = theme.content(0.08);
            let ink = theme.colors.content;
            body = body.child(
                div()
                    .h(u(36.))
                    .border_t_1()
                    .border_color(theme.colors.stroke)
                    .child(
                        div()
                            .id("workspace-settings")
                            .flex()
                            .size_full()
                            .items_center()
                            .gap(u(8.))
                            .rounded(u(theme.radius.lg))
                            .px(u(8.))
                            .text_px(13.)
                            .text_color(theme.content(0.65))
                            .hover(move |s| s.bg(hover).text_color(ink))
                            .tooltip(tooltip("Open worktree settings"))
                            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                                if *hovered {
                                    this.close_worktree_menu(cx);
                                }
                            }))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.mode_open = false;
                                this.report_open(cx);
                                this.release_focus(window, cx);
                                cx.emit(WorkspacePickerEvent::OpenSettings);
                                cx.notify();
                            }))
                            .child(
                                icon(IconName::Settings)
                                    .flex_none()
                                    .size(u(16.))
                                    .text_color(theme.content(0.45)),
                            )
                            .child(div().flex_1().child("Worktree settings")),
                    ),
            );
        }
        let trigger = self.mode_trigger.get();
        let panel = div()
            .id("workspace-popover")
            .debug_selector(|| "workspace-popover".into())
            .on_mouse_down_out(
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    // The submenu is another surface of this popover
                    // (`ignore={WORKSPACE_SURFACES}`).
                    if trigger.contains(&event.position)
                        || (this.worktree_menu && this.submenu_hovered)
                    {
                        return;
                    }
                    this.dismiss_mode(window, cx);
                }),
            )
            .child(
                popover_frame("workspace-popover-frame")
                    .side(self.props.popover_side)
                    .width(240.)
                    .animate(self.animate)
                    .child(body),
            );
        self.place(trigger, panel, cx)
    }

    fn render_worktree_menu(
        &self,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut list = div().flex().flex_col().p(u(6.));
        let loading = self.worktrees.is_none();
        if loading {
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .gap(u(8.))
                    .px(u(8.))
                    .py(u(12.))
                    .text_px(12.)
                    .text_color(theme.content(0.50))
                    .child(
                        spinner("worktrees-loading")
                            .size(14.)
                            .color(theme.content(0.50)),
                    )
                    .child("Loading worktrees…"),
            );
        }
        let trees = self.listed_worktrees();
        let mut rows = div()
            .id("worktree-rows")
            .flex()
            .flex_col()
            .min_h_0()
            .overflow_y_scroll();
        for tree in &trees {
            let busy = self.busy_path.as_deref() == Some(tree.path.as_str());
            let disabled = self.busy_path.is_some();
            let label = tree.branch.clone().unwrap_or_else(|| {
                format!("Detached {}", tree.head.chars().take(7).collect::<String>())
            });
            let hover = theme.content(0.08);
            let ink = theme.colors.content;
            let pick = tree.clone();
            let glyph: AnyElement = if busy {
                spinner(ElementId::Name(
                    format!("worktree-busy:{}", tree.path).into(),
                ))
                .size(16.)
                .color(theme.content(0.55))
                .into_any_element()
            } else {
                icon(IconName::FolderTree)
                    .flex_none()
                    .size(u(16.))
                    .text_color(theme.content(0.55))
                    .into_any_element()
            };
            rows = rows.child(
                div()
                    .id(ElementId::Name(format!("worktree:{}", tree.path).into()))
                    .debug_selector({
                        let path = tree.path.clone();
                        move || format!("worktree:{path}")
                    })
                    .flex()
                    .min_h(u(44.))
                    .w_full()
                    .items_center()
                    .gap(u(8.))
                    .rounded(u(theme.radius.lg))
                    .px(u(8.))
                    .py(u(6.))
                    .text_color(theme.content(0.80))
                    .tooltip(tooltip(tree.path.clone()))
                    .when(disabled, |row| row.opacity(0.4))
                    .when(!disabled, |row| {
                        row.hover(move |s| s.bg(hover).text_color(ink)).on_click(
                            cx.listener(move |this, _, _, cx| {
                                this.select_worktree(pick.clone(), cx)
                            }),
                        )
                    })
                    .child(glyph)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(div().truncate().text_px(13.).child(label))
                            .child(
                                div()
                                    .truncate()
                                    .font_family(theme.fonts.mono.clone())
                                    .text_px(10.)
                                    .text_color(theme.content(0.40))
                                    .child(pretty_cwd(&tree.path)),
                            ),
                    ),
            );
        }
        if matches!(self.worktrees, Some(Ok(_))) && trees.is_empty() {
            rows = rows.child(
                div()
                    .px(u(8.))
                    .py(u(12.))
                    .text_px(12.)
                    .text_color(theme.content(0.50))
                    .child("No existing worktrees"),
            );
        }
        list = list.child(rows);
        let error = self.pick_error.clone().or_else(|| {
            self.worktrees
                .as_ref()
                .and_then(|result| result.as_ref().err().cloned())
        });
        if let Some(error) = error {
            list = list.child(
                div()
                    .debug_selector(|| "worktree-error".into())
                    .border_t_1()
                    .border_color(theme.colors.stroke)
                    .px(u(8.))
                    .py(u(8.))
                    .text_px(11.)
                    .text_color(theme.colors.danger)
                    .child(error),
            );
        }
        let panel = div()
            .id("worktree-submenu")
            .debug_selector(|| "worktree-submenu".into())
            .relative()
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                this.submenu_hovered = *hovered;
                if *hovered {
                    this.open_worktree_menu(cx);
                } else {
                    this.schedule_close_worktree_menu(cx);
                }
            }))
            .child(
                popover_frame("worktree-submenu-frame")
                    .side(PopoverSide::Right)
                    .width(300.)
                    .max_height(320.)
                    .animate(self.animate)
                    .child(list),
            );
        let row = self.worktree_row.get();
        let width = u(300.).to_pixels(window.rem_size());
        // The row sits 6px inside the popover's padding; the submenu clears
        // the popover edge by `SUBMENU_GAP`.
        submenu_beside(
            row,
            px(6.0 + SUBMENU_GAP),
            px(6.),
            width,
            panel,
            window,
            theme,
        )
    }

    fn render_base_trigger(
        &self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let enabled = self.props.base_enabled();
        let selected = self.props.effective_base();
        let loading = !self.props.settled;
        let bounds = self.base_trigger.clone();
        let label: AnyElement = if loading {
            // Reserve the same line box while the current branch loads.
            div()
                .relative()
                .child(div().invisible().child("main"))
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .top(u(6.))
                        .h(u(6.))
                        .rounded_full()
                        .bg(theme.content(0.55))
                        .opacity(0.5),
                )
                .into_any_element()
        } else {
            div()
                .truncate()
                .child(format!("From {selected}"))
                .into_any_element()
        };
        let mut trigger = Self::trigger_style(theme, enabled, self.base_open, 256.)
            .id("workspace-base-trigger")
            .group("workspace-base-trigger")
            .debug_selector(|| "workspace-base-trigger".into())
            .relative()
            .tooltip(tooltip(format!("Create from {selected}")))
            .child(trigger_icon(
                "workspace-base-trigger",
                IconName::GitBranch,
                self.base_open,
                theme,
            ))
            .child(div().relative().flex_1().min_w_0().child(label))
            .child(
                canvas(move |rect, _, _| bounds.set(rect), |_, _, _, _| {})
                    .absolute()
                    .size_full(),
            )
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default());
        if enabled {
            trigger =
                trigger.on_click(cx.listener(|this, _, window, cx| this.toggle_base(window, cx)));
        }
        trigger
    }

    fn render_base_popover(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let style = InputEditorStyle {
            foreground: theme.colors.content,
            muted_foreground: theme.content(0.40),
            background: gpui::transparent_black(),
            border: gpui::transparent_black(),
            selection: theme.accent(0.35),
            caret: theme.colors.content,
            ..Default::default()
        };
        self.query
            .update(cx, |state, _| state.set_editor_style(style));
        let selected = self.props.effective_base();
        let rows = self.rows(cx);
        let mut list = div()
            .id("base-branches")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p(u(6.));
        for (index, branch) in rows.iter().enumerate() {
            let reference = branch_ref(branch);
            let is_selected = reference == selected;
            let highlighted = index == self.active;
            let hover = theme.content(0.08);
            let pick = reference.clone();
            list = list.child(
                div()
                    .id(ElementId::Name(format!("base-branch:{reference}").into()))
                    .debug_selector({
                        let reference = reference.clone();
                        move || format!("base-branch:{reference}")
                    })
                    .flex()
                    .h(u(32.))
                    .w_full()
                    .flex_none()
                    .items_center()
                    .gap(u(8.))
                    .rounded(u(theme.radius.lg))
                    .px(u(8.))
                    .text_px(13.)
                    .text_color(if highlighted || is_selected {
                        theme.colors.content
                    } else {
                        theme.content(0.80)
                    })
                    .when(highlighted || is_selected, |row| {
                        row.bg(theme.colors.selection)
                    })
                    .hover(move |s| s.bg(hover))
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        if *hovered && this.active != index {
                            this.active = index;
                            cx.notify();
                        }
                    }))
                    .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.pick_base(pick.clone(), window, cx)
                    }))
                    .child(if is_selected {
                        icon(IconName::Check)
                            .flex_none()
                            .size(u(14.))
                            .text_color(theme.colors.content)
                    } else {
                        icon(IconName::GitBranch)
                            .flex_none()
                            .size(u(14.))
                            .text_color(theme.content(0.45))
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .when(is_selected, |label| label.medium())
                            .child(reference),
                    ),
            );
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
        let trigger = self.base_trigger.get();
        let panel = div()
            .id("base-popover")
            .debug_selector(|| "base-popover".into())
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                match event.keystroke.key.as_str() {
                    "down" => {
                        cx.stop_propagation();
                        this.move_active(1, cx);
                    }
                    "up" => {
                        cx.stop_propagation();
                        this.move_active(-1, cx);
                    }
                    "escape" => {
                        cx.stop_propagation();
                        this.dismiss_base(window, cx);
                    }
                    _ => {}
                }
            }))
            .on_mouse_down_out(
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    if !trigger.contains(&event.position) {
                        this.dismiss_base(window, cx);
                    }
                }),
            )
            .child(
                popover_frame("base-popover-frame")
                    .side(self.props.popover_side)
                    .width(280.)
                    .max_height(280.)
                    .animate(self.animate)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .min_h(u(160.))
                            .max_h(u(280.))
                            .child(
                                div()
                                    .flex()
                                    .flex_none()
                                    .items_center()
                                    .gap(u(8.))
                                    .border_b_1()
                                    .border_color(theme.colors.stroke)
                                    .px(u(8.))
                                    .py(u(10.))
                                    .text_color(theme.content(0.50))
                                    .child(
                                        icon(IconName::Search)
                                            .flex_none()
                                            .size(u(14.))
                                            .text_color(theme.content(0.50)),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .h(u(18.))
                                            .text_px(12.)
                                            .child(self.query.clone()),
                                    ),
                            )
                            .child(list),
                    ),
            );
        self.place(trigger, panel, cx)
    }
}

/// A trigger's glyph: the trigger's dim ink, full ink while open or hovered.
fn trigger_icon(group: &'static str, name: IconName, open: bool, theme: &Theme) -> gpui::Svg {
    let ink = theme.colors.content;
    icon(name)
        .flex_none()
        .size(u(14.))
        .text_color(if open { ink } else { theme.content(0.55) })
        .group_hover(group, move |s| s.text_color(ink))
}

/// `WorkspaceIdentity`: a started conversation owns its working copy; only
/// its branch stays mutable.
pub fn workspace_identity(worktree: bool, theme: &Theme) -> impl IntoElement + use<> {
    let label = if worktree {
        "Worktree"
    } else {
        "Current checkout"
    };
    div()
        .id("workspace-identity")
        .flex()
        .flex_none()
        .h(u(24.))
        .min_w_0()
        .ml(u(-6.))
        .items_center()
        .gap(u(6.))
        .px(u(6.))
        .text_px(12.)
        .text_color(theme.content(0.45))
        .tooltip(tooltip(format!("Workspace: {label}")))
        .child(
            icon(if worktree {
                IconName::FolderTree
            } else {
                IconName::Folder
            })
            .flex_none()
            .size(u(14.))
            .text_color(theme.content(0.45)),
        )
        .child(div().truncate().child(label))
}

impl Render for WorkspacePicker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let mut root = div()
            .id("workspace-picker")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" && this.mode_open {
                    cx.stop_propagation();
                    if this.worktree_menu {
                        this.close_worktree_menu(cx);
                    } else {
                        this.dismiss_mode(window, cx);
                    }
                }
            }))
            .flex()
            .items_center()
            .gap(u(8.))
            .min_w_0()
            .child(self.render_mode_trigger(&theme, cx));
        if self.props.mode == WorkspaceMode::Worktree {
            root = root.child(self.render_base_trigger(&theme, cx));
        }
        if self.mode_open {
            root = root.child(self.render_mode_popover(&theme, cx));
            if self.worktree_menu {
                root = root.child(self.render_worktree_menu(&theme, window, cx));
            }
        }
        if self.base_open && self.props.mode == WorkspaceMode::Worktree {
            root = root.child(self.render_base_popover(&theme, cx));
        }
        root
    }
}

#[cfg(test)]
#[path = "workspace_picker_tests.rs"]
mod tests;
