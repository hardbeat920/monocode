//! Port of src/features/workspace/ui/SurfaceTabs.tsx: the 36px tab strip
//! of an editor or terminal pane.
//!
//! Tabs reorder by direct manipulation ([`AnimatedReorder`]), collapse when
//! they close and grow when they open ([`TabCloseMotion`]), close on a
//! middle click, and open a context menu with file actions on a right
//! click. In a split, the grip and the empty end of the strip start a pane
//! drag, which the owner forwards to [`super::pane_tree::PaneTree`].

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::Instant;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    Anchor, AnyElement, AnyView, App, Bounds, ClipboardItem, Context, CursorStyle, DispatchPhase,
    ElementId, EventEmitter, FocusHandle, InteractiveElement as _, IntoElement, KeyDownEvent,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement as _, Pixels, Point,
    Render, ScrollHandle, SharedString, StatefulInteractiveElement as _, Styled as _, Task, Window,
    anchored, canvas, deferred, div, point, px,
};
use monocode_core::paths::{basename, display_path};
use monocode_layout::terminal_tab::terminal_tab_label;
use monocode_layout::{
    FilePaneTab, is_agent_tab, is_changes_tab, is_commit_tab, is_filesystem_tab, is_plan_tab,
    is_release_notes_tab, is_review_tab, is_session_changes_tab, is_terminal_tab,
};
use monocode_ui::widgets::{MenuEntry, MenuItem, context_menu, menu, tooltip};
use monocode_ui::{
    IconName, ProviderLogo, Theme, UiStyled as _, file_type_icon, icon, provider_logo, u,
};

use super::animated_reorder::{
    AnimatedReorder, Axis, ItemSpan, OffsetTweens, PressPoint, ReorderEffect, reorder_duration,
};
use super::tab_close_motion::{TabCloseMotion, TabMotionEntry};
use super::tab_width_motion::{TabMotionPhase, tab_close_duration, tab_width_motion};

/// `SurfaceTabPresentation`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceTabPresentation {
    pub name: String,
    pub label: String,
    pub icon_name: String,
    pub tooltip: String,
}

/// `releaseNotesTitle` from src/app/model/releaseNotes.ts.
pub fn release_notes_title(version: &str) -> String {
    format!("What's new in MonoCode {version}")
}

/// `REVEAL_LABEL`.
pub const REVEAL_LABEL: &str = if cfg!(target_os = "macos") {
    "Reveal in Finder"
} else if cfg!(target_os = "windows") {
    "Reveal in File Explorer"
} else {
    "Open Containing Folder"
};

/// One `ExplorerMenuItem` of the tab menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SurfaceTabMenuItem {
    Item {
        id: &'static str,
        label: &'static str,
        /// `disabled`, which the TypeScript set on Close Others only.
        disabled: Option<bool>,
    },
    Separator,
}

fn item(id: &'static str, label: &'static str) -> SurfaceTabMenuItem {
    SurfaceTabMenuItem::Item {
        id,
        label,
        disabled: None,
    }
}

/// `surfaceTabMenuItems`.
pub fn surface_tab_menu_items(
    file: &FilePaneTab,
    can_close_others: bool,
) -> Vec<SurfaceTabMenuItem> {
    let close = item("close", "Close");
    let close_others = SurfaceTabMenuItem::Item {
        id: "close-others",
        label: "Close Others",
        disabled: Some(!can_close_others),
    };
    if !is_filesystem_tab(file) || is_changes_tab(file) {
        return vec![close, close_others];
    }
    vec![
        item("open-default", "Open in Default App"),
        item("reveal", REVEAL_LABEL),
        SurfaceTabMenuItem::Separator,
        item("copy-path", "Copy Path"),
        item("copy-relative-path", "Copy Relative Path"),
        item("copy-name", "Copy File Name"),
        SurfaceTabMenuItem::Separator,
        close,
        close_others,
    ]
}

/// `surfaceTabPresentation`.
pub fn surface_tab_presentation(file: &FilePaneTab) -> SurfaceTabPresentation {
    if is_release_notes_tab(file) {
        let version = file
            .release_notes
            .as_ref()
            .map_or("", |notes| notes.version.as_str());
        let title = release_notes_title(version);
        return SurfaceTabPresentation {
            name: title.clone(),
            label: title.clone(),
            icon_name: "CHANGELOG.md".into(),
            tooltip: title,
        };
    }
    if is_changes_tab(file) {
        return SurfaceTabPresentation {
            name: "Changes".into(),
            label: "Changes".into(),
            icon_name: "CHANGES".into(),
            tooltip: "Working tree changes".into(),
        };
    }
    if is_session_changes_tab(file) {
        return SurfaceTabPresentation {
            name: "Session Changes".into(),
            label: "Session Changes".into(),
            icon_name: "CHANGES".into(),
            tooltip: "Changes captured for this session only".into(),
        };
    }
    if is_agent_tab(file) {
        let trimmed = file.path.trim();
        let name = if trimmed.is_empty() { "Agent" } else { trimmed }.to_string();
        return SurfaceTabPresentation {
            name: name.clone(),
            label: name.clone(),
            icon_name: "AGENT".into(),
            tooltip: format!("{name} — orchestration agent"),
        };
    }
    if let (true, Some(commit)) = (is_commit_tab(file), file.commit.as_ref()) {
        let subject = commit.subject.trim();
        let name = if subject.is_empty() {
            commit.short_sha.clone()
        } else {
            subject.to_string()
        };
        return SurfaceTabPresentation {
            name: name.clone(),
            label: name,
            icon_name: "CHANGES".into(),
            tooltip: format!("{} — {}", commit.short_sha, commit.subject),
        };
    }

    let review = is_review_tab(file);
    let terminal = is_terminal_tab(file);
    let plan = is_plan_tab(file);
    let name = if plan {
        let title = file.plan.as_ref().map_or("", |plan| plan.title.trim());
        if title.is_empty() {
            "Plan".to_string()
        } else {
            title.to_string()
        }
    } else if terminal {
        terminal_tab_label(file)
    } else {
        basename(&file.path)
    };
    SurfaceTabPresentation {
        label: if review {
            format!("{name} (Working Tree)")
        } else {
            name.clone()
        },
        icon_name: if plan { "plan.md".into() } else { name.clone() },
        tooltip: if plan {
            name.clone()
        } else if terminal {
            format!("{name} — {}", file.cwd)
        } else if review {
            format!("{} (Working Tree)", file.path)
        } else {
            file.path.clone()
        },
        name,
    }
}

/// `appendProblems`: the tab tooltip, then what is wrong with the file.
pub fn append_problems(title: &str, errors: usize) -> String {
    if errors == 0 {
        return title.to_string();
    }
    format!(
        "{title} — {errors} {}",
        if errors == 1 { "problem" } else { "problems" }
    )
}

/// What the strip shows.
#[derive(Debug, Clone)]
pub struct SurfaceTabsProps {
    pub files: Vec<FilePaneTab>,
    pub active_file_id: String,
    pub dirty_file_ids: HashSet<String>,
    pub file_error_counts: HashMap<String, usize>,
    /// `onPinFile` is set: a double click makes a preview tab permanent.
    pub can_pin: bool,
    /// `onPaneDragStart` is set: the grip and the empty strip drag the pane.
    pub can_drag_pane: bool,
    /// The tab list's accessible name (`label`).
    pub label: SharedString,
    /// `monocode.tabAnimationsEnabled`.
    pub tab_animations: bool,
}

impl Default for SurfaceTabsProps {
    fn default() -> Self {
        Self {
            files: Vec::new(),
            active_file_id: String::new(),
            dirty_file_ids: HashSet::new(),
            file_error_counts: HashMap::new(),
            can_pin: false,
            can_drag_pane: false,
            label: "Open files".into(),
            tab_animations: true,
        }
    }
}

/// What the strip reports to its pane.
#[derive(Debug, Clone, PartialEq)]
pub enum SurfaceTabsEvent {
    /// `onSelectFile`.
    Select(String),
    /// `onCloseFile`.
    Close(String),
    /// `onCloseOtherFiles`.
    CloseOthers(String),
    /// `onPinFile`.
    Pin(String),
    /// `onReorder`.
    Reorder { ids: Vec<String>, moved_id: String },
    /// `onPaneDragStart`: forward to `PaneTree::start_pane_drag`.
    PaneDragStart { position: Point<Pixels> },
}

/// The file actions the context menu runs. The app implements the ones
/// that need the platform; copying uses the GPUI clipboard by default.
pub trait SurfaceTabActions {
    /// `openPathWithDefaultApp`.
    fn open_with_default_app(&self, path: &str, cx: &mut App) -> Task<Result<(), String>>;
    /// `revealPath`.
    fn reveal(&self, path: &str, cx: &mut App) -> Task<Result<(), String>>;
    /// `copyText`.
    fn copy_text(&self, text: &str, cx: &mut App) -> Task<Result<(), String>> {
        cx.write_to_clipboard(ClipboardItem::new_string(text.to_string()));
        Task::ready(Ok(()))
    }
}

/// Actions for hosts without a platform layer: opening and revealing fail.
pub struct ClipboardOnlyActions;

impl SurfaceTabActions for ClipboardOnlyActions {
    fn open_with_default_app(&self, _: &str, _: &mut App) -> Task<Result<(), String>> {
        Task::ready(Err("not supported here".into()))
    }

    fn reveal(&self, _: &str, _: &mut App) -> Task<Result<(), String>> {
        Task::ready(Err("not supported here".into()))
    }
}

/// The tab strip.
pub struct SurfaceTabs {
    props: SurfaceTabsProps,
    actions: Rc<dyn SurfaceTabActions>,
    trailing: Option<AnyView>,
    reorder: AnimatedReorder,
    tweens: OffsetTweens,
    motion: TabCloseMotion<FilePaneTab>,
    motion_timers: HashMap<String, Task<()>>,
    settle_timer: Option<Task<()>>,
    measured: Rc<RefCell<HashMap<String, Bounds<Pixels>>>>,
    menu: Option<(Point<Pixels>, String)>,
    file_action_error: Option<String>,
    scroll: ScrollHandle,
    scrolled_to: Option<String>,
    epoch: Instant,
    focus: FocusHandle,
    restore_focus: Option<FocusHandle>,
}

impl EventEmitter<SurfaceTabsEvent> for SurfaceTabs {}

impl SurfaceTabs {
    pub fn new(
        props: SurfaceTabsProps,
        actions: Rc<dyn SurfaceTabActions>,
        cx: &mut Context<Self>,
    ) -> Self {
        let motion = TabCloseMotion::new(&props.files);
        Self {
            props,
            actions,
            trailing: None,
            reorder: AnimatedReorder::new(Axis::X),
            tweens: OffsetTweens::default(),
            motion,
            motion_timers: HashMap::new(),
            settle_timer: None,
            measured: Rc::default(),
            menu: None,
            file_action_error: None,
            scroll: ScrollHandle::new(),
            scrolled_to: None,
            epoch: cx.background_executor().now(),
            focus: cx.focus_handle(),
            restore_focus: None,
        }
    }

    pub fn props(&self) -> &SurfaceTabsProps {
        &self.props
    }

    /// New props from the pane. Closed and new tabs start their motion here.
    pub fn set_props(&mut self, props: SurfaceTabsProps, cx: &mut Context<Self>) {
        let skip_motion = cx.reduce_motion() || !props.tab_animations;
        {
            let measured = self.measured.borrow();
            for (id, bounds) in measured.iter() {
                self.motion.record_width(id, f32::from(bounds.size.width));
            }
        }
        let started = self.motion.sync(&props.files, skip_motion);
        if self
            .menu
            .as_ref()
            .is_some_and(|(_, id)| !props.files.iter().any(|file| file.id == *id))
        {
            self.menu = None;
        }
        self.props = props;
        let duration = tab_close_duration(Theme::of(cx));
        for id in started {
            let timer_id = id.clone();
            let task = cx.spawn(async move |this, cx| {
                cx.background_executor().timer(duration).await;
                this.update(cx, |this, cx| {
                    this.motion.finish_motion(&timer_id);
                    this.motion_timers.remove(&timer_id);
                    cx.notify();
                })
                .ok();
            });
            self.motion_timers.insert(id, task);
        }
        cx.notify();
    }

    /// `trailing`: controls after the tab list.
    pub fn set_trailing(&mut self, trailing: Option<AnyView>, cx: &mut Context<Self>) {
        self.trailing = trailing;
        cx.notify();
    }

    /// The entries drawn: live tabs and tabs mid-motion.
    pub fn displayed(&self) -> Vec<TabMotionEntry<FilePaneTab>> {
        self.motion.displayed(&self.props.files)
    }

    pub fn file_action_error(&self) -> Option<&str> {
        self.file_action_error.as_deref()
    }

    pub fn menu_file_id(&self) -> Option<&str> {
        self.menu.as_ref().map(|(_, id)| id.as_str())
    }

    fn now_ms(&self, cx: &App) -> f64 {
        cx.background_executor()
            .now()
            .saturating_duration_since(self.epoch)
            .as_secs_f64()
            * 1000.0
    }

    fn ids(&self) -> Vec<String> {
        self.props
            .files
            .iter()
            .map(|file| file.id.clone())
            .collect()
    }

    fn apply(&mut self, effects: Vec<ReorderEffect>, window: &mut Window, cx: &mut Context<Self>) {
        for effect in effects {
            match effect {
                ReorderEffect::Reorder { ids, moved_id } => {
                    cx.emit(SurfaceTabsEvent::Reorder { ids, moved_id })
                }
                ReorderEffect::End { .. } => {
                    self.settle_timer = None;
                    self.tweens.clear();
                    self.release_keyboard(window, cx);
                }
            }
        }
        if let Some(deadline) = self.reorder.settle_deadline() {
            let wait = (deadline - self.now_ms(cx)).max(0.0);
            self.settle_timer = Some(cx.spawn_in(window, async move |this, cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs_f64(wait / 1000.0))
                    .await;
                this.update_in(cx, |this, window, cx| {
                    let now = this.now_ms(cx);
                    let effects = this.reorder.tick(now);
                    this.apply(effects, window, cx);
                })
                .ok();
            }));
        }
        cx.notify();
    }

    fn release_keyboard(&mut self, window: &mut Window, cx: &mut App) {
        if let Some(previous) = self.restore_focus.take() {
            window.focus(&previous, cx);
        } else if self.focus.is_focused(window) {
            window.blur();
        }
    }

    /// `onPointerDown` on a tab: select it and arm the reorder gesture.
    fn press_tab(
        &mut self,
        id: &str,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.emit(SurfaceTabsEvent::Select(id.to_string()));
        let ids = self.ids();
        let spans: Vec<Option<ItemSpan>> = {
            let measured = self.measured.borrow();
            ids.iter()
                .map(|id| {
                    measured.get(id).map(|bounds| {
                        ItemSpan::new(f32::from(bounds.origin.x), f32::from(bounds.size.width))
                    })
                })
                .collect()
        };
        let duration = reorder_duration(Theme::of(cx), cx.reduce_motion());
        let now = self.now_ms(cx);
        let scroll = -f32::from(self.scroll.offset().x);
        let effects = self.reorder.press(
            id,
            &ids,
            &spans,
            PressPoint {
                position: f32::from(position.x),
                scroll,
            },
            duration,
            now,
        );
        self.apply(effects, window, cx);
    }

    fn pointer_moved(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let was_grabbing = self.reorder.is_grabbing();
        if self.reorder.pointer_move(f32::from(position.x), false) {
            if !was_grabbing && self.reorder.is_grabbing() {
                self.menu = None;
                if self.restore_focus.is_none() {
                    self.restore_focus = window.focused(cx);
                }
                window.focus(&self.focus, cx);
            }
            cx.notify();
        }
    }

    fn pointer_released(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let now = self.now_ms(cx);
        let effects = self
            .reorder
            .release(f32::from(position.x), false, false, now);
        self.apply(effects, window, cx);
    }

    fn cancel_drag(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let now = self.now_ms(cx);
        let effects = self.reorder.cancel(now);
        self.apply(effects, window, cx);
    }

    /// `onMenuPick`.
    fn pick_menu(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some((_, file_id)) = self.menu.take() else {
            return;
        };
        let Some(file) = self
            .props
            .files
            .iter()
            .find(|file| file.id == file_id)
            .cloned()
        else {
            return;
        };
        self.file_action_error = None;
        cx.notify();
        match id {
            "close" => return cx.emit(SurfaceTabsEvent::Close(file.id)),
            "close-others" => return cx.emit(SurfaceTabsEvent::CloseOthers(file.id)),
            _ => {}
        }
        if !is_filesystem_tab(&file) || is_changes_tab(&file) {
            return;
        }
        let actions = self.actions.clone();
        let action = match id {
            "open-default" => actions.open_with_default_app(&file.path, cx),
            "reveal" => actions.reveal(&file.path, cx),
            "copy-path" => actions.copy_text(&file.path, cx),
            "copy-relative-path" => {
                actions.copy_text(&display_path(&file.path, Some(&file.cwd)), cx)
            }
            "copy-name" => actions.copy_text(&basename(&file.path), cx),
            _ => return,
        };
        let open_default = id == "open-default";
        let action_id = id.to_string();
        cx.spawn_in(window, async move |this, cx| {
            if let Err(error) = action.await {
                eprintln!("Failed to run file-tab action {action_id}: {error}");
                this.update(cx, |this, cx| {
                    this.file_action_error = Some(format!(
                        "Could not {}: {error}",
                        if open_default {
                            "open the file in its default app"
                        } else {
                            "complete the file action"
                        }
                    ));
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    fn render_tab(
        &self,
        entry: &TabMotionEntry<FilePaneTab>,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let file = &entry.item;
        let id = file.id.clone();
        let closing = entry.closing;
        let active = !closing && file.id == self.props.active_file_id;
        let dirty = self.props.dirty_file_ids.contains(&file.id);
        let errors = self
            .props
            .file_error_counts
            .get(&file.id)
            .copied()
            .unwrap_or(0);
        let changes = is_changes_tab(file);
        let commit = is_commit_tab(file);
        let review = is_review_tab(file) && !changes;
        let terminal = is_terminal_tab(file);
        let agent = file.agent.as_ref().filter(|_| is_agent_tab(file));
        let presentation = surface_tab_presentation(file);
        let group = SharedString::from(format!("surface-tab-group:{id}"));
        // `[data-reordering]` hides the close buttons while the tab follows
        // the pointer; the settle animation shows them again.
        let reordering = self.reorder.is_grabbing();

        // Glyphs take the label's color, brightening with the tab on hover.
        let glyph_ink = if active {
            theme.colors.content
        } else {
            theme.content(0.50)
        };
        let ink = theme.colors.content;
        let tinted = |name: IconName| {
            icon(name)
                .flex_none()
                .size(u(14.))
                .text_color(glyph_ink)
                .group_hover(group.clone(), move |s| s.text_color(ink))
                .into_any_element()
        };
        let glyph: AnyElement = if terminal {
            tinted(IconName::Terminal)
        } else if let Some(agent) = agent {
            match ProviderLogo::from_id(agent.harness.as_str()) {
                Some(logo) => provider_logo(logo).size(14.).into_any_element(),
                None => tinted(IconName::Bot),
            }
        } else if changes || commit || review {
            tinted(IconName::GitCompare)
        } else {
            file_type_icon(presentation.icon_name.clone())
                .size(14.)
                .into_any_element()
        };

        let label_color = if errors > 0 {
            if active {
                Some(theme.colors.danger)
            } else {
                Some(monocode_ui::color::with_alpha(theme.colors.danger, 0.75))
            }
        } else {
            None
        };
        let mut label = div()
            .flex_1()
            .min_w_0()
            .truncate()
            .child(presentation.label.clone());
        if file.preview == Some(true) {
            label = label.italic();
        }
        if let Some(color) = label_color {
            label = label.text_color(color);
            if !active {
                let danger = theme.colors.danger;
                label = label.group_hover(group.clone(), move |s| s.text_color(danger));
            }
        }

        let tip = append_problems(&presentation.tooltip, errors);
        let mut button = div()
            .id(ElementId::Name(format!("surface-tab-button:{id}").into()))
            .debug_selector({
                let id = id.clone();
                move || format!("surface-tab-button:{id}")
            })
            .relative()
            .flex()
            .flex_1()
            .min_w_0()
            .h(u(30.))
            .items_center()
            .gap(u(6.))
            .rounded(u(theme.radius.md))
            .px(u(8.))
            .pr(u(28.))
            .text_px(13.)
            .cursor(CursorStyle::Arrow)
            .child(glyph)
            .child(label)
            .when(dirty, |button| {
                button.child(
                    div()
                        .flex_none()
                        .size(u(6.))
                        .rounded_full()
                        .bg(theme.content(0.70)),
                )
            });
        button = if active {
            button
                .bg(theme.colors.selection)
                .text_color(theme.colors.content)
        } else {
            let hover_bg = theme.content(0.05);
            let ink = theme.colors.content;
            button
                .text_color(theme.content(0.50))
                .hover(move |s| s.bg(hover_bg).text_color(ink))
        };
        if !closing {
            button = button.tooltip(tooltip(tip));
            let select_id = id.clone();
            let can_pin = self.props.can_pin;
            button = button.on_click(cx.listener(move |this, event: &gpui::ClickEvent, _, cx| {
                if event.click_count() == 2 && can_pin {
                    cx.emit(SurfaceTabsEvent::Pin(select_id.clone()));
                    return;
                }
                if this.reorder.consume_click(this.now_ms(cx)) {
                    return;
                }
                cx.emit(SurfaceTabsEvent::Select(select_id.clone()));
            }));
        }

        let close_label = format!("Close {}", presentation.label);
        let close_id = id.clone();
        let close_group = SharedString::from(format!("surface-tab-close-group:{id}"));
        let mut close = div()
            .id(ElementId::Name(format!("surface-tab-close:{id}").into()))
            .group(close_group.clone())
            .debug_selector({
                let id = id.clone();
                move || format!("surface-tab-close:{id}")
            })
            .flex()
            .size(u(20.))
            .items_center()
            .justify_center()
            .rounded(u(theme.radius.sm))
            .text_color(theme.content(0.50))
            .hover({
                let fill = theme.content(0.10);
                let ink = theme.colors.content;
                move |s| s.bg(fill).text_color(ink)
            })
            .child(
                icon(IconName::X)
                    .size(u(12.))
                    .text_color(theme.content(0.50))
                    .group_hover(close_group.clone(), {
                        let ink = theme.colors.content;
                        move |s| s.text_color(ink)
                    }),
            );
        if !active {
            close = close
                .opacity(0.)
                .group_hover(group.clone(), |s| s.opacity(1.));
        }
        if reordering {
            close = close.invisible();
        }
        if !closing {
            close = close
                .tooltip(tooltip(close_label))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |_, _, _, cx| {
                    cx.stop_propagation();
                    cx.emit(SurfaceTabsEvent::Close(close_id.clone()));
                }));
        }

        let moving = closing || entry.opening;
        let mut slot = div()
            .id(ElementId::Name(format!("surface-tab:{id}").into()))
            .debug_selector({
                let id = id.clone();
                move || format!("surface-tab:{id}")
            })
            .group(group)
            .relative()
            .flex()
            .h_full()
            .min_w_0()
            .items_center()
            .child(button)
            .child(
                div()
                    .absolute()
                    .right(u(4.))
                    .top_0()
                    .bottom_0()
                    .flex()
                    .items_center()
                    .child(close),
            );
        slot = if moving {
            slot.w_full().overflow_hidden()
        } else {
            slot.w(u(224.)).min_w(u(112.)).flex_shrink(1.)
        };
        if !closing {
            let measured = self.measured.clone();
            let measure_id = id.clone();
            slot = slot.child(
                canvas(
                    move |bounds, _, _| {
                        measured.borrow_mut().insert(measure_id, bounds);
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            );
            let offset = self.tweens.value(&id, cx.background_executor().now());
            if offset != 0.0 {
                slot = slot.left(px(offset));
            }
            let press_id = id.clone();
            let middle_id = id.clone();
            let menu_id = id.clone();
            slot = slot
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                        this.press_tab(&press_id, event.position, window, cx);
                    }),
                )
                .on_aux_click(cx.listener(move |_, event: &gpui::ClickEvent, _, cx| {
                    if event.is_middle_click() {
                        cx.stop_propagation();
                        cx.emit(SurfaceTabsEvent::Close(middle_id.clone()));
                    }
                }))
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        cx.emit(SurfaceTabsEvent::Select(menu_id.clone()));
                        this.file_action_error = None;
                        this.menu = Some((event.position, menu_id.clone()));
                        cx.notify();
                    }),
                );
        }
        if moving {
            let phase = if closing {
                TabMotionPhase::Closing
            } else {
                TabMotionPhase::Opening
            };
            return tab_width_motion(
                ElementId::Name(format!("surface-tab-motion:{id}:{phase:?}").into()),
                phase,
                entry.width,
                slot.into_any_element(),
                theme,
            );
        }
        slot.into_any_element()
    }

    fn render_menu(&self, theme: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (position, file_id) = self.menu.clone()?;
        let file = self.props.files.iter().find(|file| file.id == file_id)?;
        let entries: Vec<MenuEntry> = surface_tab_menu_items(file, self.props.files.len() > 1)
            .into_iter()
            .map(|entry| match entry {
                SurfaceTabMenuItem::Separator => MenuEntry::Separator,
                SurfaceTabMenuItem::Item {
                    id,
                    label,
                    disabled,
                } => MenuItem::new(id, label)
                    .disabled(disabled == Some(true))
                    .into(),
            })
            .collect();
        let pick = cx.entity().downgrade();
        let dismiss = cx.entity().downgrade();
        let _ = theme;
        Some(
            context_menu(
                position,
                menu("surface-tab-menu", entries).on_pick(move |id, window, cx| {
                    let id = id.to_string();
                    pick.update(cx, |this, cx| this.pick_menu(&id, window, cx))
                        .ok();
                }),
                move |_, cx| {
                    dismiss
                        .update(cx, |this, cx| {
                            this.menu = None;
                            cx.notify();
                        })
                        .ok();
                },
                cx,
            )
            .into_any_element(),
        )
    }

    fn render_error(
        &self,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let message = self.file_action_error.clone()?;
        let viewport = window.viewport_size();
        let ink = theme.colors.content;
        Some(
            deferred(
                anchored()
                    .anchor(Anchor::BottomRight)
                    .position(point(
                        viewport.width - u(16.).to_pixels(window.rem_size()),
                        viewport.height - u(16.).to_pixels(window.rem_size()),
                    ))
                    .child(
                        div()
                            .debug_selector(|| "file-action-error".into())
                            .flex()
                            .max_w(u(384.))
                            .items_start()
                            .gap(u(12.))
                            .rounded(u(theme.radius.xl))
                            .border_1()
                            .border_color(monocode_ui::color::with_alpha(theme.colors.danger, 0.3))
                            .bg(monocode_ui::color::hex(0x252525))
                            .px(u(12.))
                            .py(u(8.))
                            .text_px(12.)
                            .text_color(theme.colors.danger_soft)
                            .shadow_xl()
                            .child(div().min_w_0().child(message))
                            .child(
                                div()
                                    .id("file-action-error-dismiss")
                                    .flex_none()
                                    .text_color(theme.content(0.60))
                                    .hover(move |s| s.text_color(ink))
                                    .child("Dismiss")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.file_action_error = None;
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
            .with_priority(theme.layer.toast)
            .into_any_element(),
        )
    }

    fn drag_listeners(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let strip = cx.entity().downgrade();
        let settling = self.reorder.is_settling();
        canvas(
            |_, _, _| {},
            move |_, _, window, _| {
                if settling {
                    // Commit before a new press can change the list,
                    // including close buttons.
                    let finish = strip.clone();
                    window.on_mouse_event(move |_: &MouseDownEvent, phase, window, cx| {
                        if phase != DispatchPhase::Capture {
                            return;
                        }
                        finish
                            .update(cx, |this, cx| {
                                let now = this.now_ms(cx);
                                let effects = this.reorder.finish_now(now);
                                this.apply(effects, window, cx);
                            })
                            .ok();
                    });
                    return;
                }
                let on_move = strip.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                    if phase == DispatchPhase::Capture {
                        on_move
                            .update(cx, |this, cx| {
                                this.pointer_moved(event.position, window, cx)
                            })
                            .ok();
                    }
                });
                let on_up = strip.clone();
                window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
                    if phase == DispatchPhase::Capture && event.button == MouseButton::Left {
                        on_up
                            .update(cx, |this, cx| {
                                this.pointer_released(event.position, window, cx)
                            })
                            .ok();
                    }
                });
            },
        )
        .absolute()
        .size_full()
    }
}

impl Render for SurfaceTabs {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let now = cx.background_executor().now();

        // Follow the strip's scroll while a tab drags.
        if self.reorder.is_grabbing() {
            self.reorder.scroll(-f32::from(self.scroll.offset().x));
        }
        // Point each tab at its reorder offset; the transition plays the move.
        if self.reorder.is_pressed() {
            let duration = std::time::Duration::from_secs_f64(
                reorder_duration(&theme, cx.reduce_motion()) / 1000.0,
            );
            for file in &self.props.files {
                let target = self.reorder.offset(&file.id).unwrap_or(0.0);
                self.tweens.target(
                    &file.id,
                    target,
                    self.reorder.animates(&file.id),
                    duration,
                    theme.motion.ease_out,
                    now,
                );
            }
        } else {
            self.tweens.clear();
        }
        if self.tweens.is_animating(now) || self.motion.is_moving() {
            window.request_animation_frame();
        }

        let displayed = self.displayed();
        // `scrollIntoView` for the active tab, unless a tab is dragging.
        if self.reorder.dragging_id().is_none()
            && self.scrolled_to.as_deref() != Some(&self.props.active_file_id)
        {
            let offset = usize::from(self.props.can_drag_pane);
            if let Some(index) = displayed
                .iter()
                .position(|entry| entry.id == self.props.active_file_id)
            {
                self.scroll.scroll_to_item(index + offset);
                self.scrolled_to = Some(self.props.active_file_id.clone());
            }
        }

        let mut strip = div()
            .id("surface-tabs-strip")
            .debug_selector(|| "surface-tabs-strip".into())
            .flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .items_center()
            .gap(u(2.))
            .overflow_x_scroll()
            .track_scroll(&self.scroll)
            .pl(u(6.))
            .pr(u(10.));
        if self.props.can_drag_pane {
            let hover_bg = theme.content(0.05);
            let hover_ink = theme.content(0.70);
            strip = strip.child(
                div()
                    .id("surface-tabs-grip")
                    .group("surface-tabs-grip")
                    .debug_selector(|| "surface-tabs-grip".into())
                    .flex()
                    .flex_none()
                    .h(u(30.))
                    .w(u(20.))
                    .items_center()
                    .justify_center()
                    .rounded(u(theme.radius.md))
                    .text_color(theme.content(0.35))
                    .cursor(CursorStyle::OpenHand)
                    .hover(move |s| s.bg(hover_bg).text_color(hover_ink))
                    .tooltip(tooltip("Drag to reorder pane"))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|_, event: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            cx.emit(SurfaceTabsEvent::PaneDragStart {
                                position: event.position,
                            });
                        }),
                    )
                    .child(
                        icon(IconName::GripVertical)
                            .size(u(14.))
                            .text_color(theme.content(0.35))
                            .group_hover("surface-tabs-grip", move |s| s.text_color(hover_ink)),
                    ),
            );
        }
        for entry in &displayed {
            strip = strip.child(self.render_tab(entry, &theme, cx));
        }
        if self.props.can_drag_pane {
            strip = strip.child(
                div()
                    .id("surface-tabs-spacer")
                    .min_w(u(16.))
                    .flex_1()
                    .h_full()
                    .cursor(CursorStyle::OpenHand)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|_, event: &MouseDownEvent, _, cx| {
                            cx.emit(SurfaceTabsEvent::PaneDragStart {
                                position: event.position,
                            });
                        }),
                    ),
            );
        }

        let mut root = div()
            .id("surface-tabs")
            .key_context("SurfaceTabs")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" && this.reorder.is_grabbing() {
                    cx.stop_propagation();
                    this.cancel_drag(window, cx);
                }
            }))
            .relative()
            .flex()
            .flex_none()
            .h(u(36.))
            .min_w_0()
            .border_b_1()
            .border_color(theme.colors.stroke)
            .child(strip);
        if let Some(trailing) = self.trailing.clone() {
            root = root.child(trailing);
        }
        if self.reorder.is_pressed() {
            root = root.child(self.drag_listeners(cx));
        }
        if let Some(menu) = self.render_menu(&theme, cx) {
            root = root.child(menu);
        }
        if let Some(error) = self.render_error(&theme, window, cx) {
            root = root.child(error);
        }
        root
    }
}

#[cfg(test)]
#[path = "surface_tabs_tests.rs"]
mod tests;
