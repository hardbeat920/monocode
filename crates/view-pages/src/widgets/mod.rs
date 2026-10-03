//! Small pieces the pages repeat: the page title bar, tabs, the compact
//! toggle, text fields, the project mark, the project picker, and the
//! markdown preview and source toggle.

pub mod markdown_mode;
pub mod mascot;
pub mod project_mark;
pub mod project_picker;
pub mod provider_mark;

use std::rc::Rc;

use gpui::{
    AnyElement, App, ClickEvent, ElementId, Entity, Hsla, InteractiveElement as _, IntoElement,
    ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_base::input::{InputBaseState, InputEditorStyle, InputModeKind};
use monocode_ui::widgets::{icon_button, tooltip};
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

pub use markdown_mode::{MarkdownMode, MarkdownModes, markdown_mode_toggle};
pub use project_mark::project_mark;
pub use project_picker::{ProjectPicker, ProjectPickerAppearance, ProjectPickerMode};

type ClickFn = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// The unframed gpui-base input in page colors: content text, a `content/40`
/// placeholder unless `placeholder` overrides it, and the accent selection.
/// Wrap it in a sized div that sets the font.
pub fn plain_input<M: InputModeKind + 'static>(
    state: &Entity<InputBaseState<M>>,
    placeholder: Option<Hsla>,
    cx: &mut App,
) -> AnyElement {
    let theme = Theme::of(cx);
    let style = InputEditorStyle {
        foreground: theme.colors.content,
        muted_foreground: placeholder.unwrap_or_else(|| theme.content(0.40)),
        background: gpui::transparent_black(),
        border: gpui::transparent_black(),
        selection: theme.accent(0.35),
        caret: theme.colors.content,
        ..Default::default()
    };
    state.update(cx, |state, _| state.set_editor_style(style));
    state.clone().into_any_element()
}

/// Where a page sits, for its title bar: the macOS traffic light spacer and
/// the back and sidebar buttons (`OverlayNav`).
#[derive(Clone, Default)]
pub struct PageChrome {
    /// The page sits beside the project rail, which already clears the
    /// traffic lights and holds the navigation.
    pub beside_rail: bool,
    /// The compact rail is showing (`w-4` extra inset on macOS).
    pub compact_rail: bool,
    pub on_back: Option<ClickFn>,
    pub on_toggle_sidebar: Option<ClickFn>,
}

impl PageChrome {
    pub fn on_back(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_back = Some(Rc::new(f));
        self
    }

    pub fn on_toggle_sidebar(
        mut self,
        f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_toggle_sidebar = Some(Rc::new(f));
        self
    }
}

/// The page title bar: `flex h-10 items-center border-b border-stroke`,
/// with a 14px `content/45` icon and the 13px title, then `trailing`.
pub fn page_title_bar(
    chrome: &PageChrome,
    glyph: IconName,
    title: impl IntoElement,
    theme: &Theme,
) -> gpui::Div {
    let mac = cfg!(target_os = "macos");
    let mut bar = div()
        .flex()
        .flex_none()
        .h(u(theme.metrics.title_bar_height))
        .items_center()
        .border_b_1()
        .border_color(theme.colors.stroke);
    if mac && chrome.compact_rail {
        bar = bar.child(div().flex_none().w(u(16.)));
    }
    if mac && !chrome.beside_rail {
        bar = bar.child(div().flex_none().w(u(theme.metrics.traffic_light_inset)));
    }
    if !chrome.beside_rail && (chrome.on_back.is_some() || chrome.on_toggle_sidebar.is_some()) {
        let mut nav = div().flex().flex_none().items_center().px(u(6.));
        if let Some(back) = chrome.on_back.clone() {
            nav = nav.child(
                icon_button("page-back", IconName::ChevronLeft)
                    .tooltip(format!("Back ({}[)", modifier_label()))
                    .on_click(move |event, window, cx| back(event, window, cx)),
            );
        }
        if let Some(toggle) = chrome.on_toggle_sidebar.clone() {
            nav = nav.child(
                icon_button("page-toggle-sidebar", IconName::PanelLeft)
                    .tooltip(format!("Toggle Sidebar ({}B)", modifier_label()))
                    .on_click(move |event, window, cx| toggle(event, window, cx)),
            );
        }
        bar = bar.child(nav);
    }
    bar.child(
        div()
            .flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .gap(u(8.))
            .px(u(12.))
            .text_px(theme.text.body)
            .child(icon(glyph).size(u(14.)).text_color(theme.content(0.45)))
            .child(div().min_w_0().flex_1().flex().items_center().child(title)),
    )
}

/// `MOD` in TitleBar.tsx.
fn modifier_label() -> &'static str {
    if cfg!(target_os = "macos") {
        "⌘"
    } else {
        "Ctrl+"
    }
}

/// `PageTab` / `NoteDetailTab`: an `h-9` 12px tab with a 2px content
/// underline when selected.
pub fn page_tab(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    selected: bool,
    theme: &Theme,
) -> gpui::Stateful<gpui::Div> {
    let ink = theme.colors.content;
    div()
        .id(id)
        .relative()
        .flex()
        .h(u(36.))
        .items_center()
        .text_px(theme.text.label)
        .leading(theme.leading.none)
        .map(|tab| {
            if selected {
                tab.text_color(ink)
            } else {
                tab.text_color(theme.content(0.50))
                    .hover(move |s| s.text_color(ink))
            }
        })
        .child(label.into())
        .when(selected, |tab| {
            tab.child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .h(u(2.))
                    .bg(ink),
            )
        })
}

/// AutomationsView's `ToggleSwitch`: `h-5 w-9`, or `h-4 w-7` compact, accent
/// when on, with `label` as its tooltip.
pub fn toggle_switch(
    id: impl Into<ElementId>,
    on: bool,
    compact: bool,
    label: impl Into<SharedString>,
    theme: &Theme,
) -> gpui::Stateful<gpui::Div> {
    let label: SharedString = label.into();
    switch_body(id, on, compact, theme).tooltip(tooltip(label))
}

/// The `h-5 w-9` switch SkillsPage draws, without a tooltip.
pub fn switch_track(
    id: impl Into<ElementId>,
    on: bool,
    theme: &Theme,
) -> gpui::Stateful<gpui::Div> {
    switch_body(id, on, false, theme)
}

fn switch_body(
    id: impl Into<ElementId>,
    on: bool,
    compact: bool,
    theme: &Theme,
) -> gpui::Stateful<gpui::Div> {
    let (width, height, knob, travel) = if compact {
        (28., 16., 12., 12.)
    } else {
        (36., 20., 16., 16.)
    };
    div()
        .id(id)
        .relative()
        .flex_none()
        .w(u(width))
        .h(u(height))
        .rounded_full()
        .bg(if on {
            theme.colors.accent
        } else {
            theme.content(0.20)
        })
        .child(
            div()
                .absolute()
                .top(u(2.))
                .left(u(2. + if on { travel } else { 0. }))
                .size(u(knob))
                .rounded_full()
                .bg(gpui::white()),
        )
}

/// Puts `child` in a box the size of the window at the window's origin, so
/// a modal rendered from a view embedded in a page still covers the whole
/// window.
pub fn window_overlay(window: &Window, child: impl IntoElement) -> impl IntoElement {
    let size = window.viewport_size();
    gpui::anchored()
        .position(gpui::point(px(0.), px(0.)))
        .child(div().w(size.width).h(size.height).child(child))
}

/// A thin vertical divider (`h-3 w-px bg-content/15`).
pub fn vertical_rule(theme: &Theme) -> gpui::Div {
    div()
        .flex_none()
        .h(u(12.))
        .w(px(1.))
        .bg(theme.content(0.15))
}

/// The `LoaderCircle animate-spin` glyph.
pub fn spinner_icon(id: impl Into<ElementId>, size: f32, color: Hsla) -> impl IntoElement {
    use gpui::{Animation, AnimationExt as _, Transformation, percentage};
    icon(IconName::LoaderCircle)
        .size(u(size))
        .text_color(color)
        .with_animation(
            id,
            Animation::new(std::time::Duration::from_secs(1)).repeat(),
            |svg, delta| svg.with_transformation(Transformation::rotate(percentage(delta))),
        )
}
