//! The controls the Connect views share: the `button`, `input`, and `code`
//! class strings at the top of ConnectionsSettings.tsx, and the dialog
//! buttons from AddRemoteProjectDialog.tsx and RemoteSession.tsx.

use std::rc::Rc;

use gpui::{
    AnyElement, App, ClickEvent, ElementId, Entity, Focusable as _, Hsla, InteractiveElement as _,
    IntoElement, ParentElement as _, RenderOnce, SharedString, StatefulInteractiveElement as _,
    Styled as _, StyledText, Window, div,
};
use gpui_base::input::InputEditorStyle;
use gpui_component::input::InputState;
use monocode_ui::widgets::tooltip;
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// How a button is filled.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ButtonKind {
    /// `bg-selection hover:bg-selection-hover`.
    #[default]
    Filled,
    /// `text-content/50`, no fill: the Cancel buttons in Settings.
    Quiet,
    /// `text-content/70 hover:bg-content/8 hover:text-content`: the dialog's
    /// Cancel and the notice action.
    Ghost,
}

/// A text button. The default is the `button` class in Settings:
/// `rounded-lg bg-selection px-3 py-2 text-[13px] font-medium`.
#[derive(IntoElement)]
pub struct ActionButton {
    id: ElementId,
    label: SharedString,
    selector: String,
    icon: Option<IconName>,
    kind: ButtonKind,
    small: bool,
    medium: bool,
    radius: Option<f32>,
    disabled: bool,
    ink: Option<Hsla>,
    tooltip: Option<SharedString>,
    on_click: Option<ClickHandler>,
}

/// A button whose test selector is `button:<label>`.
pub fn action_button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> ActionButton {
    let label = label.into();
    ActionButton {
        id: id.into(),
        selector: format!("button:{label}"),
        label,
        icon: None,
        kind: ButtonKind::Filled,
        small: false,
        medium: true,
        radius: None,
        disabled: false,
        ink: None,
        tooltip: None,
        on_click: None,
    }
}

impl ActionButton {
    pub fn kind(mut self, kind: ButtonKind) -> Self {
        self.kind = kind;
        if kind != ButtonKind::Filled {
            self.medium = false;
        }
        self
    }

    /// `px-3 py-1.5 text-[12px]`, as the dialogs and the session pane draw
    /// their buttons.
    pub fn small(mut self) -> Self {
        self.small = true;
        self
    }

    pub fn medium(mut self, medium: bool) -> Self {
        self.medium = medium;
        self
    }

    /// Corner radius in CSS px. The default is `rounded-lg`, or `rounded-md`
    /// for ghost buttons.
    pub fn radius(mut self, radius: f32) -> Self {
        self.radius = Some(radius);
        self
    }

    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Text color in place of the kind's ink.
    pub fn ink(mut self, ink: Hsla) -> Self {
        self.ink = Some(ink);
        self
    }

    pub fn tooltip(mut self, text: impl Into<SharedString>) -> Self {
        self.tooltip = Some(text.into());
        self
    }

    /// Replaces the `button:<label>` test selector.
    pub fn selector(mut self, selector: impl Into<String>) -> Self {
        self.selector = selector.into();
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for ActionButton {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let c = theme.colors;
        let (fill, ink, hover_fill, hover_ink) = match self.kind {
            ButtonKind::Filled => (Some(c.selection), c.content, Some(c.selection_hover), None),
            ButtonKind::Quiet => (None, theme.content(0.50), None, None),
            ButtonKind::Ghost => (
                None,
                theme.content(0.70),
                Some(theme.content(0.08)),
                Some(c.content),
            ),
        };
        let ink = self.ink.unwrap_or(ink);
        let radius = self.radius.unwrap_or(match self.kind {
            ButtonKind::Ghost => theme.radius.md,
            _ => theme.radius.lg,
        });
        let selector = self.selector;
        let mut el = div()
            .id(self.id)
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .gap(u(8.))
            .px(u(12.))
            .py(u(if self.small { 6. } else { 8. }))
            .rounded(u(radius))
            .text_px(if self.small {
                theme.text.label
            } else {
                theme.text.body
            })
            .leading(theme.leading.normal)
            .whitespace_nowrap()
            .text_color(ink)
            .debug_selector(move || selector);
        if let Some(fill) = fill {
            el = el.bg(fill);
        }
        if self.medium {
            el = el.medium();
        }
        if self.disabled {
            el = el.opacity(0.4);
        } else {
            el = el.hover(move |s| {
                let mut s = s;
                if let Some(fill) = hover_fill {
                    s = s.bg(fill);
                }
                if let Some(ink) = hover_ink {
                    s = s.text_color(ink);
                }
                s
            });
        }
        if let Some(name) = self.icon {
            el = el.child(icon(name).size(u(16.)).text_color(ink));
        }
        el = el.child(self.label);
        if let Some(text) = self.tooltip {
            el = el.tooltip(tooltip(text));
        }
        if let (Some(handler), false) = (self.on_click, self.disabled) {
            el = el.on_click(move |event, window, cx| handler(event, window, cx));
        }
        el
    }
}

/// Applies the views' colors to an input: content text, `content/35`
/// placeholder, accent selection.
pub fn style_input(state: &Entity<InputState>, cx: &mut App) {
    let theme = Theme::of(cx).clone();
    let style = InputEditorStyle {
        foreground: theme.colors.content,
        muted_foreground: theme.content(0.35),
        background: gpui::transparent_black(),
        border: gpui::transparent_black(),
        selection: theme.accent(0.35),
        caret: theme.colors.content,
        ..Default::default()
    };
    state.update(cx, |state, _| state.set_editor_style(style));
}

/// How a text field is drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FieldStyle {
    /// Height in CSS px.
    pub height: f32,
    pub padding_x: f32,
    pub radius: f32,
    pub text: f32,
    pub border: f32,
    pub focus_border: f32,
    pub mono: bool,
}

impl FieldStyle {
    /// The `input` class in Settings: `rounded-lg border-content/15
    /// bg-content/3 px-3 py-2 text-[13px]`, `focus:border-content/35`.
    pub fn settings() -> Self {
        Self {
            height: 36.,
            padding_x: 12.,
            radius: 8.,
            text: 13.,
            border: 0.15,
            focus_border: 0.35,
            mono: false,
        }
    }

    /// The folder path field: `h-8 rounded-md border-content/10 px-2.5
    /// font-mono text-[12px]`, `focus:border-content/25`.
    pub fn path() -> Self {
        Self {
            height: 32.,
            padding_x: 10.,
            radius: 6.,
            text: 12.,
            border: 0.10,
            focus_border: 0.25,
            mono: true,
        }
    }

    pub fn mono(mut self) -> Self {
        self.mono = true;
        self
    }
}

/// A bordered text field around `state`. Its test selector is `selector`.
pub fn text_input(
    selector: impl Into<String>,
    state: &Entity<InputState>,
    style: FieldStyle,
    window: &Window,
    cx: &mut App,
) -> gpui::Div {
    let theme = Theme::of(cx).clone();
    let focused = state.read(cx).focus_handle(cx).is_focused(window);
    style_input(state, cx);
    let selector = selector.into();
    let mut field = div()
        .flex()
        .flex_none()
        .items_center()
        .w_full()
        .h(u(style.height))
        .px(u(style.padding_x))
        .rounded(u(style.radius))
        .border_1()
        .border_color(theme.content(if focused {
            style.focus_border
        } else {
            style.border
        }))
        .bg(theme.content(0.03))
        .text_px(style.text)
        .text_color(theme.colors.content)
        .debug_selector(move || selector)
        .child(div().flex_1().min_w_0().child(state.clone()));
    if style.mono {
        field = field.font_family(theme.fonts.mono.clone());
    }
    field
}

/// Text with inline `code` spans: `rounded bg-content/10 px-1` in the
/// monospace face. GPUI draws the background behind the glyphs only, so the
/// span has no padding or rounding.
#[derive(IntoElement)]
pub struct CodeText {
    spans: Vec<(SharedString, bool)>,
}

/// Plain text.
pub fn text(value: impl Into<SharedString>) -> (SharedString, bool) {
    (value.into(), false)
}

/// A `code` span.
pub fn code(value: impl Into<SharedString>) -> (SharedString, bool) {
    (value.into(), true)
}

pub fn code_text(spans: impl IntoIterator<Item = (SharedString, bool)>) -> CodeText {
    CodeText {
        spans: spans.into_iter().collect(),
    }
}

impl RenderOnce for CodeText {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let style = window.text_style();
        let mut full = String::new();
        let mut runs = Vec::new();
        for (value, is_code) in &self.spans {
            if value.is_empty() {
                continue;
            }
            full.push_str(value);
            let mut run = style.to_run(value.len());
            if *is_code {
                run.font.family = theme.fonts.mono.clone();
                run.background_color = Some(theme.content(0.10));
            }
            runs.push(run);
        }
        StyledText::new(full).with_runs(runs)
    }
}

/// `label.flex.flex-col.gap-1.5.text-[12px].text-content/65`: a caption
/// over a field.
pub fn field_label(caption: impl IntoElement, field: impl IntoElement, cx: &App) -> gpui::Div {
    let theme = Theme::of(cx);
    div()
        .flex()
        .flex_col()
        .gap(u(6.))
        .text_px(theme.text.label)
        .text_color(theme.content(0.65))
        .child(caption)
        .child(field)
}

/// `text-[12px] leading-relaxed text-content/45`: a help paragraph.
pub fn help(content: impl IntoElement, cx: &App) -> AnyElement {
    let theme = Theme::of(cx);
    div()
        .text_px(theme.text.label)
        .leading(theme.leading.relaxed)
        .text_color(theme.content(0.45))
        .child(content)
        .into_any_element()
}

/// `Loader` with `animate-spin`: one turn per second.
pub fn spinning_loader(id: impl Into<ElementId>, size: f32, color: Hsla) -> AnyElement {
    use gpui::{Animation, AnimationExt as _, Transformation, percentage};
    icon(IconName::Loader)
        .size(u(size))
        .text_color(color)
        .with_animation(
            id,
            Animation::new(std::time::Duration::from_secs(1)).repeat(),
            |svg, t| svg.with_transformation(Transformation::rotate(percentage(t))),
        )
        .into_any_element()
}

/// Text whose spans each have their own color, such as a notice with a
/// dimmer detail. GPUI blends highlight colors over the base color, so this
/// sets each run's color outright.
#[derive(IntoElement)]
pub struct TintedText {
    spans: Vec<(SharedString, Option<Hsla>)>,
}

/// Spans with `None` keep the inherited text color.
pub fn tinted_text(spans: impl IntoIterator<Item = (SharedString, Option<Hsla>)>) -> TintedText {
    TintedText {
        spans: spans.into_iter().collect(),
    }
}

impl RenderOnce for TintedText {
    fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
        let style = window.text_style();
        let mut full = String::new();
        let mut runs = Vec::new();
        for (value, color) in &self.spans {
            if value.is_empty() {
                continue;
            }
            full.push_str(value);
            let mut run = style.to_run(value.len());
            if let Some(color) = color {
                run.color = *color;
            }
            runs.push(run);
        }
        StyledText::new(full).with_runs(runs)
    }
}
