//! Port of src/features/workspace/ui/TabGroupMenu.tsx: the menu a tab
//! group's chip opens. It renames the group, sets its project logo, color,
//! and mascot, and runs the group actions, plus any extra rows the owner
//! adds (such as notification muting with a submenu).
//!
//! Picking a logo and the custom color picker belong to the owner: the menu
//! reports [`TabGroupMenuEvent::PickLogo`] and shows the owner's picker view
//! when the custom swatch is open.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    Anchor, AnyElement, AnyView, App, AppContext as _, Bounds, Context, ElementId, Entity,
    EventEmitter, Focusable as _, Hsla, InteractiveElement as _, IntoElement, KeyDownEvent,
    MouseButton, MouseDownEvent, ParentElement as _, Pixels, Point, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Task, Window, canvas, div, img, px,
};
use gpui_base::input::InputEditorStyle;
use gpui_component::input::{InputEvent, InputState};
use monocode_layout::tab_groups::TAB_GROUP_COLORS;
use monocode_ui::color::{hsl_to_rgb, parse_hex, with_alpha};
use monocode_ui::widgets::{popover_at, popover_frame};
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

use super::pixel_art::{PROJECT_MASCOTS, project_mascot, sprite};
use super::submenu::submenu_beside;

/// `MENU_WIDTH`.
pub const MENU_WIDTH: f32 = 260.0;
/// How long the submenu waits after the pointer leaves both panels.
const SUBMENU_CLOSE_MS: u64 = 180;

/// `TabGroupMenuAction`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TabGroupMenuAction {
    NewTab,
    NewWindow,
    CloseGroup,
    Ungroup,
    DeleteGroup,
}

impl TabGroupMenuAction {
    pub fn id(self) -> &'static str {
        match self {
            Self::NewTab => "new-tab",
            Self::NewWindow => "new-window",
            Self::CloseGroup => "close-group",
            Self::Ungroup => "ungroup",
            Self::DeleteGroup => "delete-group",
        }
    }
}

/// One entry of an extra row's submenu (`ExplorerMenuItem`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubmenuEntry {
    Item {
        id: SharedString,
        label: SharedString,
        disabled: bool,
        checked: bool,
    },
    Separator,
}

/// `TabGroupMenuExtraItem`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabGroupMenuExtraItem {
    pub id: SharedString,
    pub label: SharedString,
    pub description: Option<SharedString>,
    pub icon: IconName,
    pub danger: bool,
    pub sep_before: bool,
    pub disabled: bool,
    pub submenu: Option<Vec<SubmenuEntry>>,
}

impl TabGroupMenuExtraItem {
    pub fn new(
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        icon: IconName,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            description: None,
            icon,
            danger: false,
            sep_before: false,
            disabled: false,
            submenu: None,
        }
    }
}

/// `MenuItem` and the fixed `ITEMS`.
#[derive(Debug, Clone)]
struct Row {
    id: SharedString,
    label: SharedString,
    description: Option<SharedString>,
    shortcut: Option<SharedString>,
    icon: IconName,
    danger: bool,
    disabled: bool,
    submenu: bool,
    /// One of the group actions; other rows are the owner's extra items.
    action: Option<TabGroupMenuAction>,
}

fn mod_key() -> &'static str {
    if cfg!(target_os = "macos") {
        "⌘"
    } else {
        "Ctrl+"
    }
}

fn action_row(action: TabGroupMenuAction) -> Row {
    let (label, shortcut, icon, danger) = match action {
        TabGroupMenuAction::NewTab => (
            "New tab in group",
            Some(format!("{}T", mod_key())),
            IconName::SquarePlus,
            false,
        ),
        TabGroupMenuAction::NewWindow => {
            ("Move group to new window", None, IconName::AppWindow, false)
        }
        TabGroupMenuAction::CloseGroup => (
            "Close group",
            Some(format!("{}W", mod_key())),
            IconName::X,
            false,
        ),
        TabGroupMenuAction::Ungroup => ("Ungroup", None, IconName::Ungroup, false),
        TabGroupMenuAction::DeleteGroup => ("Delete group", None, IconName::Trash2, true),
    };
    Row {
        id: action.id().into(),
        label: label.into(),
        description: None,
        shortcut: shortcut.map(Into::into),
        icon,
        danger,
        disabled: false,
        submenu: false,
        action: Some(action),
    }
}

fn extra_row(item: &TabGroupMenuExtraItem) -> Row {
    Row {
        id: item.id.clone(),
        label: item.label.clone(),
        description: item.description.clone(),
        shortcut: None,
        icon: item.icon,
        danger: item.danger,
        disabled: item.disabled,
        submenu: item.submenu.is_some(),
        action: None,
    }
}

/// Parses `hsl(H S% L%)`, the form `TAB_GROUP_COLORS` uses, or `#rrggbb`.
pub fn parse_css_color(value: &str) -> Option<Hsla> {
    if let Some(color) = parse_hex(value) {
        return Some(color);
    }
    let inner = value.trim().strip_prefix("hsl(")?.strip_suffix(')')?;
    let mut parts = inner
        .split_whitespace()
        .map(|part| part.trim_end_matches('%').parse::<f64>());
    let (h, s, l) = (
        parts.next()?.ok()?,
        parts.next()?.ok()?,
        parts.next()?.ok()?,
    );
    Some(hsl_to_rgb(h, s, l).to_hsla())
}

/// What the menu shows.
#[derive(Clone)]
pub struct TabGroupMenuProps {
    /// Window position of the click that opened it.
    pub position: Point<Pixels>,
    pub group_id: String,
    pub label: String,
    pub color_index: Option<usize>,
    pub custom_color: Option<String>,
    /// The group's color now, as `#rrggbb`, for the custom picker.
    pub current_color: String,
    pub logo_path: Option<String>,
    /// Original project directory for the logo picker.
    pub logo_project: Option<String>,
    /// Explicit mascot pick; `None` means the one hashed from
    /// `mascot_project`.
    pub mascot_name: Option<String>,
    /// Key the fallback mascot is hashed from, the same one the icon uses.
    pub mascot_project: String,
    /// When false, only the name, logo, and color controls show.
    pub show_actions: bool,
    /// An active state action shown before the name and appearance controls.
    pub leading_action: Option<TabGroupMenuExtraItem>,
    pub extra_items: Vec<TabGroupMenuExtraItem>,
}

impl Default for TabGroupMenuProps {
    fn default() -> Self {
        Self {
            position: Point::default(),
            group_id: String::new(),
            label: String::new(),
            color_index: None,
            custom_color: None,
            current_color: "#7c3aed".into(),
            logo_path: None,
            logo_project: None,
            mascot_name: None,
            mascot_project: String::new(),
            show_actions: true,
            leading_action: None,
            extra_items: Vec::new(),
        }
    }
}

/// What the menu reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TabGroupMenuEvent {
    /// `onRename`, on Enter or when the name field loses focus.
    Rename { group_id: String, label: String },
    /// `onColorChange`: `None` is the neutral gray.
    ColorChange {
        group_id: String,
        color_index: Option<usize>,
    },
    /// The custom swatch opened or closed its picker.
    CustomPickerToggled(bool),
    /// `onMascotChange`.
    MascotChange {
        group_id: String,
        name: Option<String>,
    },
    /// The logo button: the owner runs `pickAndSetProjectLogo`.
    PickLogo { project: String },
    /// `clearProjectLogo`.
    ClearLogo { project: String },
    /// `onPick`.
    Pick(TabGroupMenuAction),
    /// `onExtraPick`.
    ExtraPick(String),
    /// `onClose`.
    Close,
}

/// Decides whether an extra pick closes the menu. Return false to keep it
/// open after a validation or persistence error.
pub type ExtraPickHandler = Rc<dyn Fn(&str, &mut Window, &mut App) -> bool>;

/// The tab group menu.
pub struct TabGroupMenu {
    props: TabGroupMenuProps,
    name: Entity<InputState>,
    custom_picker_open: bool,
    custom_picker: Option<AnyView>,
    /// The extra row whose submenu is open.
    submenu: Option<SharedString>,
    submenu_close: Option<Task<()>>,
    row_bounds: Rc<RefCell<HashMap<SharedString, Bounds<Pixels>>>>,
    /// The pointer is over the submenu, which counts as inside the menu.
    submenu_hovered: bool,
    on_extra_pick: Option<ExtraPickHandler>,
    animate: bool,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<TabGroupMenuEvent> for TabGroupMenu {}

impl TabGroupMenu {
    pub fn new(props: TabGroupMenuProps, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let label = props.label.clone();
        let name = cx.new(|cx| InputState::new(window, cx).default_value(label));
        let subscriptions =
            vec![
                cx.subscribe_in(
                    &name,
                    window,
                    |this, _, event: &InputEvent, window, cx| match event {
                        InputEvent::PressEnter { .. } => {
                            this.commit_name(cx);
                            this.close(window, cx);
                        }
                        InputEvent::Blur => this.commit_name(cx),
                        _ => {}
                    },
                ),
            ];
        // The name field takes focus with its text selected.
        name.update(cx, |state, cx| {
            state.focus(window, cx);
            state.select_all(window, cx);
        });
        Self {
            props,
            name,
            custom_picker_open: false,
            custom_picker: None,
            submenu: None,
            submenu_close: None,
            row_bounds: Rc::default(),
            submenu_hovered: false,
            on_extra_pick: None,
            animate: true,
            _subscriptions: subscriptions,
        }
    }

    pub fn props(&self) -> &TabGroupMenuProps {
        &self.props
    }

    pub fn set_props(&mut self, props: TabGroupMenuProps, cx: &mut Context<Self>) {
        self.props = props;
        cx.notify();
    }

    pub fn set_on_extra_pick(&mut self, handler: Option<ExtraPickHandler>) {
        self.on_extra_pick = handler;
    }

    /// The owner's color picker, shown under the swatches while the custom
    /// swatch is open (`ColorPickerPopover`).
    pub fn set_custom_picker(&mut self, picker: Option<AnyView>, cx: &mut Context<Self>) {
        self.custom_picker = picker;
        cx.notify();
    }

    /// Turns the open animation off, for screenshots.
    pub fn set_animate(&mut self, animate: bool) {
        self.animate = animate;
    }

    pub fn custom_picker_open(&self) -> bool {
        self.custom_picker_open
    }

    /// Opens an extra row's submenu, as hovering the row does.
    pub fn show_submenu(&mut self, id: &str, cx: &mut Context<Self>) {
        let item = self
            .props
            .extra_items
            .iter()
            .find(|item| item.id.as_ref() == id && item.submenu.is_some() && !item.disabled);
        let next = item.map(|item| item.id.clone());
        self.set_submenu(next, cx);
    }

    /// The extra row whose submenu is open.
    pub fn open_submenu(&self) -> Option<&str> {
        self.submenu.as_ref().map(|id| id.as_ref())
    }

    /// The name field's text.
    pub fn name(&self, cx: &App) -> String {
        self.name.read(cx).value().to_string()
    }

    fn commit_name(&mut self, cx: &mut Context<Self>) {
        let label = self.name.read(cx).value().trim().to_string();
        cx.emit(TabGroupMenuEvent::Rename {
            group_id: self.props.group_id.clone(),
            label,
        });
    }

    fn close(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(TabGroupMenuEvent::Close);
    }

    fn pick_extra(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        cx.emit(TabGroupMenuEvent::ExtraPick(id.to_string()));
        let keep_open = self
            .on_extra_pick
            .clone()
            .is_some_and(|handler| !handler(id, window, cx));
        if !keep_open {
            self.close(window, cx);
        }
    }

    fn set_submenu(&mut self, submenu: Option<SharedString>, cx: &mut Context<Self>) {
        self.submenu_close = None;
        if self.submenu != submenu {
            self.submenu = submenu;
            cx.notify();
        }
    }

    /// `cancelSubmenuClose`.
    fn cancel_submenu_close(&mut self) {
        self.submenu_close = None;
    }

    /// `scheduleSubmenuClose`.
    fn schedule_submenu_close(&mut self, cx: &mut Context<Self>) {
        self.submenu_close = None;
        if self.submenu.is_none() {
            return;
        }
        self.submenu_close = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(SUBMENU_CLOSE_MS))
                .await;
            this.update(cx, |this, cx| {
                this.submenu_close = None;
                this.submenu = None;
                cx.notify();
            })
            .ok();
        }));
    }

    fn hover_row(&mut self, row: &Row, cx: &mut Context<Self>) {
        let extra = self.props.extra_items.iter().find(|item| item.id == row.id);
        let next = extra
            .filter(|item| item.submenu.is_some() && !item.disabled)
            .map(|item| item.id.clone());
        self.set_submenu(next, cx);
    }

    fn pick_row(&mut self, row: &Row, window: &mut Window, cx: &mut Context<Self>) {
        if row.disabled {
            return;
        }
        if row.submenu {
            self.set_submenu(Some(row.id.clone()), cx);
            return;
        }
        match row.action {
            Some(action) => cx.emit(TabGroupMenuEvent::Pick(action)),
            None => self.pick_extra(&row.id.clone(), window, cx),
        }
    }

    fn render_row(&self, row: Row, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let expanded = self.submenu.as_ref() == Some(&row.id);
        let (ink, hover) = if row.disabled {
            (theme.content(0.30), None)
        } else if row.danger {
            (
                with_alpha(theme.colors.danger_soft, 0.9),
                Some(with_alpha(theme.colors.danger_fill, 0.15)),
            )
        } else {
            (theme.colors.content, Some(theme.content(0.05)))
        };
        let id = row.id.clone();
        let mut element = div()
            .id(ElementId::Name(format!("tab-group-row:{id}").into()))
            .debug_selector({
                let id = id.clone();
                move || format!("tab-group-row:{id}")
            })
            .flex()
            .w_full()
            .min_h(u(32.))
            .items_center()
            .gap(u(10.))
            .rounded(u(theme.radius.lg))
            .px(u(8.))
            .text_px(13.)
            .leading(theme.leading.none)
            .text_color(ink)
            .when(expanded, |el| el.bg(theme.colors.selection))
            .when_some(hover, |el, fill| el.hover(move |s| s.bg(fill)))
            .child(
                icon(row.icon)
                    .flex_none()
                    .size(u(14.))
                    .text_color(theme.content(0.55)),
            );
        let mut label = div().flex_1().min_w_0();
        label = match row.description.clone() {
            Some(description) => label
                .py(u(8.))
                .leading(theme.leading.label)
                .child(row.label.clone())
                .child(
                    div()
                        .mt(u(4.))
                        .text_px(11.)
                        .leading(theme.leading.snug)
                        .text_color(theme.content(0.60))
                        .child(description),
                ),
            None => label
                .truncate()
                .leading(theme.leading.label)
                .child(row.label.clone()),
        };
        element = element.child(label);
        if row.submenu {
            element = element.child(
                icon(IconName::ChevronRight)
                    .flex_none()
                    .size(u(14.))
                    .text_color(theme.content(0.50)),
            );
            let bounds = self.row_bounds.clone();
            let measured_id = id.clone();
            element = element.child(
                canvas(
                    move |rect, _, _| {
                        bounds.borrow_mut().insert(measured_id, rect);
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            );
        }
        if let Some(shortcut) = row.shortcut.clone() {
            element = element.child(
                div()
                    .flex_none()
                    .text_px(11.)
                    .text_color(theme.content(0.40))
                    .child(shortcut),
            );
        }
        let hover_row = row.clone();
        let pick_row = row.clone();
        element
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if *hovered {
                    this.hover_row(&hover_row, cx);
                }
            }))
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
            .on_click(cx.listener(move |this, _, window, cx| this.pick_row(&pick_row, window, cx)))
            .into_any_element()
    }

    fn separator(theme: &Theme) -> impl IntoElement + use<> {
        div().my(u(4.)).h(px(1.)).bg(theme.content(0.10))
    }

    fn render_name_field(
        &self,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let style = InputEditorStyle {
            foreground: theme.colors.content,
            muted_foreground: theme.content(0.35),
            background: gpui::transparent_black(),
            border: gpui::transparent_black(),
            selection: theme.accent(0.35),
            caret: theme.colors.content,
            ..Default::default()
        };
        self.name
            .update(cx, |state, _| state.set_editor_style(style));
        let focused = self.name.read(cx).focus_handle(cx).is_focused(window);
        div()
            .debug_selector(|| "tab-group-name".into())
            .mb(u(8.))
            .w_full()
            .h(u(32.))
            .flex()
            .items_center()
            .rounded(u(theme.radius.lg))
            .border_1()
            .border_color(if focused {
                theme.accent(0.40)
            } else {
                theme.content(0.10)
            })
            .bg(theme.content(0.05))
            .px(u(10.))
            .text_px(13.)
            .text_color(theme.colors.content)
            .child(self.name.clone())
    }

    fn render_logo(&self, theme: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let project = self.props.logo_project.clone()?;
        let logo = self.props.logo_path.clone();
        let label = if logo.is_some() {
            "Change project logo"
        } else {
            "Add project logo"
        };
        let pick_project = project.clone();
        let glyph: AnyElement = match &logo {
            Some(path) => img(std::path::PathBuf::from(path))
                .size(u(20.))
                .into_any_element(),
            None => icon(IconName::ImagePlus)
                .size(u(20.))
                .text_color(theme.content(0.70))
                .into_any_element(),
        };
        let hover = theme.content(0.10);
        let mut row = div()
            .mb(u(8.))
            .flex()
            .items_center()
            .gap(u(8.))
            .px(u(2.))
            .child(
                div()
                    .id("tab-group-logo")
                    .flex()
                    .flex_none()
                    .size(u(36.))
                    .items_center()
                    .justify_center()
                    .rounded(u(theme.radius.lg))
                    .border_1()
                    .border_color(theme.content(0.10))
                    .bg(theme.content(0.05))
                    .hover(move |s| s.bg(hover))
                    .tooltip(monocode_ui::widgets::tooltip(label))
                    .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.emit(TabGroupMenuEvent::PickLogo {
                            project: pick_project.clone(),
                        });
                        this.close(window, cx);
                    }))
                    .child(glyph),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_px(11.)
                            .text_color(theme.content(0.50))
                            .child("Project logo"),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_px(12.)
                            .text_color(theme.content(0.70))
                            .child(if logo.is_some() {
                                "Shown in tabs and composer"
                            } else {
                                "Optional — replaces folder icon"
                            }),
                    ),
            );
        if logo.is_some() {
            let ink = theme.colors.content;
            row = row.child(
                div()
                    .id("tab-group-logo-clear")
                    .group("tab-group-logo-clear")
                    .flex()
                    .flex_none()
                    .size(u(28.))
                    .items_center()
                    .justify_center()
                    .rounded(u(theme.radius.md))
                    .text_color(theme.content(0.50))
                    .hover(move |s| s.bg(hover).text_color(ink))
                    .tooltip(monocode_ui::widgets::tooltip("Remove project logo"))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.emit(TabGroupMenuEvent::ClearLogo {
                            project: project.clone(),
                        });
                    }))
                    .child(
                        icon(IconName::Trash2)
                            .size(u(14.))
                            .text_color(theme.content(0.50))
                            .group_hover("tab-group-logo-clear", move |s| s.text_color(ink)),
                    ),
            );
        }
        Some(row.into_any_element())
    }

    /// `ColorSwatchRow`.
    fn render_swatches(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let custom = self.props.custom_color.clone();
        let mut row = div()
            .mb(u(8.))
            .flex()
            .items_center()
            .justify_between()
            .gap(u(4.))
            .px(u(2.));
        for (index, color) in TAB_GROUP_COLORS.iter().enumerate() {
            let fill = parse_css_color(color).unwrap_or(theme.colors.accent);
            let selected = custom.is_none()
                && (self.props.color_index == Some(index)
                    || (self.props.color_index.is_none() && index == 0));
            let group_id = self.props.group_id.clone();
            row = row.child(
                swatch(
                    ElementId::Name(format!("tab-group-color:{index}").into()),
                    selected,
                    theme,
                    div().size_full().rounded_full().bg(fill),
                )
                .tooltip(monocode_ui::widgets::tooltip(format!(
                    "Color {}",
                    index + 1
                )))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.custom_picker_open = false;
                    cx.emit(TabGroupMenuEvent::CustomPickerToggled(false));
                    cx.emit(TabGroupMenuEvent::ColorChange {
                        group_id: group_id.clone(),
                        color_index: if index == 0 { None } else { Some(index) },
                    });
                    cx.notify();
                })),
            );
        }
        let pipette_active = custom.is_some() || self.custom_picker_open;
        let face: AnyElement = match custom.as_deref().and_then(parse_css_color) {
            Some(color) => div()
                .size_full()
                .rounded_full()
                .bg(color)
                .into_any_element(),
            None => hue_wheel().into_any_element(),
        };
        row.child(
            swatch("tab-group-color:custom", pipette_active, theme, face)
                .tooltip(monocode_ui::widgets::tooltip("Custom color"))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.custom_picker_open = !this.custom_picker_open;
                    cx.emit(TabGroupMenuEvent::CustomPickerToggled(
                        this.custom_picker_open,
                    ));
                    cx.notify();
                })),
        )
    }

    fn render_mascots(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let shown = project_mascot(
            &self.props.mascot_project,
            self.props.mascot_name.as_deref(),
        )
        .name;
        let mut row = div().flex().items_center().justify_between().gap(u(4.));
        for mascot in PROJECT_MASCOTS {
            let selected = shown == mascot.name;
            let group_id = self.props.group_id.clone();
            let hover = theme.content(0.08);
            let mut button = div()
                .id(ElementId::Name(
                    format!("tab-group-mascot:{}", mascot.name).into(),
                ))
                .flex()
                .flex_none()
                .size(u(20.))
                .items_center()
                .justify_center()
                .rounded(u(theme.radius.md))
                .tooltip(monocode_ui::widgets::tooltip(mascot.name))
                .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
                .on_click(cx.listener(move |_, _, _, cx| {
                    cx.emit(TabGroupMenuEvent::MascotChange {
                        group_id: group_id.clone(),
                        name: Some(mascot.name.to_string()),
                    });
                }))
                .child(sprite(mascot.rest, theme.content(0.75)).size(u(12.)));
            button = if selected {
                button
                    .bg(theme.colors.selection_hover)
                    .border_1()
                    .border_color(theme.content(0.50))
            } else {
                button.hover(move |s| s.bg(hover))
            };
            row = row.child(button);
        }
        div()
            .mb(u(8.))
            .px(u(2.))
            .child(
                div()
                    .mb(u(4.))
                    .text_px(11.)
                    .text_color(theme.content(0.50))
                    .child("Mascot"),
            )
            .child(row)
    }

    fn render_submenu(
        &self,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let id = self.submenu.clone()?;
        let item = self.props.extra_items.iter().find(|item| item.id == id)?;
        let entries = item.submenu.clone()?;
        let anchor = self.row_bounds.borrow().get(&id).copied()?;
        let mut list = div().flex().flex_col().p(u(4.));
        for (index, entry) in entries.into_iter().enumerate() {
            match entry {
                SubmenuEntry::Separator => {
                    list = list.child(div().my(u(4.)).h(px(1.)).bg(theme.content(0.10)));
                }
                SubmenuEntry::Item {
                    id,
                    label,
                    disabled,
                    checked,
                } => {
                    let hover = theme.content(0.05);
                    let pick = id.clone();
                    list = list.child(
                        div()
                            .id(ElementId::Name(format!("tab-group-submenu:{index}").into()))
                            .debug_selector({
                                let id = id.clone();
                                move || format!("tab-group-submenu-item:{id}")
                            })
                            .flex()
                            .h(u(28.))
                            .items_center()
                            .px(u(8.))
                            .rounded(u(theme.radius.lg))
                            .text_px(theme.text.body)
                            .text_color(if disabled {
                                theme.content(0.30)
                            } else {
                                theme.colors.content
                            })
                            .when(!disabled, |row| {
                                row.hover(move |s| s.bg(hover)).on_click(cx.listener(
                                    move |this, _, window, cx| {
                                        this.pick_extra(&pick.clone(), window, cx);
                                    },
                                ))
                            })
                            .child(label)
                            .when(checked, |row| {
                                row.child(icon(IconName::Check).ml(u(8.)).size(u(12.)))
                            }),
                    );
                }
            }
        }
        let panel = div()
            .id("tab-group-submenu")
            .debug_selector(|| "tab-group-submenu".into())
            .relative()
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                this.submenu_hovered = *hovered;
                if *hovered {
                    this.cancel_submenu_close();
                } else {
                    this.schedule_submenu_close(cx);
                }
            }))
            .child(
                popover_frame(ElementId::Name(
                    format!("tab-group-submenu-frame:{id}").into(),
                ))
                .width(monocode_ui::widgets::MENU_WIDTH)
                .animate(self.animate)
                .child(list),
            );
        let width = u(monocode_ui::widgets::MENU_WIDTH).to_pixels(window.rem_size());
        // The row sits 8px inside the menu's padding.
        Some(submenu_beside(
            anchor,
            px(12.),
            px(4.),
            width,
            panel,
            window,
            theme,
        ))
    }
}

/// A 20px round swatch button with the selection ring.
fn swatch(
    id: impl Into<ElementId>,
    selected: bool,
    theme: &Theme,
    face: impl IntoElement,
) -> gpui::Stateful<gpui::Div> {
    let ring = div()
        .flex()
        .size(u(18.))
        .items_center()
        .justify_center()
        .rounded_full()
        .when(selected, |ring| {
            ring.border_2().border_color(theme.content(0.80))
        })
        .child(
            div()
                .size(u(14.))
                .rounded_full()
                .overflow_hidden()
                .child(face),
        );
    div()
        .id(id)
        .flex()
        .flex_none()
        .size(u(20.))
        .items_center()
        .justify_center()
        .rounded_full()
        .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
        .child(ring)
}

/// The custom swatch's `conic-gradient` with a pipette. GPUI draws linear
/// gradients only, so the hue turns as six horizontal bands.
fn hue_wheel() -> impl IntoElement {
    let hues = [0.0, 60.0, 120.0, 180.0, 240.0, 300.0, 360.0];
    let mut band = div().relative().size_full().flex();
    for pair in hues.windows(2) {
        let from = hsl_to_rgb(pair[0], 100.0, 50.0).to_hsla();
        let to = hsl_to_rgb(pair[1], 100.0, 50.0).to_hsla();
        band = band.child(div().flex_1().h_full().bg(gpui::linear_gradient(
            90.,
            gpui::linear_color_stop(from, 0.),
            gpui::linear_color_stop(to, 1.),
        )));
    }
    band.child(
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(
                icon(IconName::Pipette)
                    .size(u(8.))
                    .text_color(gpui::white()),
            ),
    )
}

impl Render for TabGroupMenu {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let mut body = div().flex().flex_col().p(u(8.));
        if let Some(lead) = self.props.leading_action.clone() {
            body = body
                .child(self.render_row(extra_row(&lead), &theme, cx))
                .child(Self::separator(&theme));
        }
        body = body.child(self.render_name_field(&theme, window, cx));
        if let Some(logo) = self.render_logo(&theme, cx) {
            body = body.child(logo);
        }
        body = body.child(self.render_swatches(&theme, cx));
        if self.custom_picker_open
            && let Some(picker) = self.custom_picker.clone()
        {
            body = body.child(div().mb(u(8.)).child(picker));
        }
        body = body.child(self.render_mascots(&theme, cx));
        if self.props.show_actions {
            use TabGroupMenuAction::*;
            body = body.child(Self::separator(&theme));
            for action in [NewTab, NewWindow] {
                body = body.child(self.render_row(action_row(action), &theme, cx));
            }
            body = body.child(Self::separator(&theme));
            for action in [CloseGroup, Ungroup] {
                body = body.child(self.render_row(action_row(action), &theme, cx));
            }
            body = body.child(Self::separator(&theme));
            body = body.child(self.render_row(action_row(DeleteGroup), &theme, cx));
        }
        if !self.props.extra_items.is_empty() {
            body = body.child(Self::separator(&theme));
            for item in self.props.extra_items.clone() {
                if item.sep_before {
                    body = body.child(Self::separator(&theme));
                }
                body = body.child(self.render_row(extra_row(&item), &theme, cx));
            }
        }

        let panel = div()
            .id("tab-group-menu")
            .debug_selector(|| "tab-group-menu".into())
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key != "escape" {
                    return;
                }
                cx.stop_propagation();
                if this.submenu.is_some() {
                    this.set_submenu(None, cx);
                } else {
                    this.close(window, cx);
                }
            }))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if *hovered {
                    this.cancel_submenu_close();
                } else {
                    this.schedule_submenu_close(cx);
                }
            }))
            .on_mouse_down_out(cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                if this.submenu.is_some() && this.submenu_hovered {
                    return;
                }
                this.close(window, cx);
            }))
            .child(
                popover_frame("tab-group-menu-frame")
                    .side(monocode_ui::widgets::PopoverSide::Right)
                    .width(MENU_WIDTH)
                    .animate(self.animate)
                    .child(body),
            );
        let mut root = div().child(popover_at(self.props.position, Anchor::TopLeft, panel, cx));
        if let Some(submenu) = self.render_submenu(&theme, window, cx) {
            root = root.child(submenu);
        }
        root
    }
}

#[cfg(test)]
#[path = "tab_group_menu_tests.rs"]
mod tests;
