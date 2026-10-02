//! Buttons. Ports of `.primary-action` (index.css), SecondaryButton.tsx, and
//! the icon buttons the shell repeats (TitleBar.tsx `IconButton`, the
//! sidebar's `size-6` and `size-5` header buttons).

use std::rc::Rc;

use gpui::{
    App, ClickEvent, CursorStyle, ElementId, InteractiveElement as _, IntoElement,
    ParentElement as _, RenderOnce, SharedString, StatefulInteractiveElement as _, Styled as _,
    Window, div,
};

use crate::styled::UiStyled as _;
use crate::widgets::tooltip::tooltip;
use crate::{IconName, Theme, icon, u};

type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ButtonVariant {
    /// The solid `.primary-action` (white in dark mode, the user accent when set).
    Primary,
    /// `SecondaryButton`: bordered, quiet ink.
    #[default]
    Secondary,
    /// No border; `bg-selection` when selected.
    Ghost,
    /// `SecondaryButton danger`.
    Danger,
}

/// A labeled button with an optional leading icon.
#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    label: SharedString,
    icon: Option<IconName>,
    variant: ButtonVariant,
    disabled: bool,
    selected: bool,
    full_width: bool,
    compact: bool,
    tooltip: Option<SharedString>,
    on_click: Option<ClickHandler>,
}

pub fn button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Button {
    Button {
        id: id.into(),
        label: label.into(),
        icon: None,
        variant: ButtonVariant::default(),
        disabled: false,
        selected: false,
        full_width: false,
        compact: false,
        tooltip: None,
        on_click: None,
    }
}

impl Button {
    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn primary(self) -> Self {
        self.variant(ButtonVariant::Primary)
    }

    pub fn ghost(self) -> Self {
        self.variant(ButtonVariant::Ghost)
    }

    pub fn danger(self) -> Self {
        self.variant(ButtonVariant::Danger)
    }

    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn full_width(mut self) -> Self {
        self.full_width = true;
        self
    }

    /// The 11px, `h-5` size used in toasts and chips.
    pub fn compact(mut self) -> Self {
        self.compact = true;
        self
    }

    pub fn tooltip(mut self, text: impl Into<SharedString>) -> Self {
        self.tooltip = Some(text.into());
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

impl RenderOnce for Button {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let c = theme.colors;
        let text = if self.compact {
            theme.text.caption
        } else {
            theme.text.label
        };
        let disabled = self.disabled;
        let selected = self.selected;
        // (fill, ink, hover fill, hover ink, border, hover border)
        let none = gpui::transparent_black();
        let (fill, ink, hover_fill, hover_ink, border, hover_border) = match self.variant {
            ButtonVariant::Primary if disabled => (
                c.primary_disabled,
                c.primary_disabled_foreground,
                None,
                None,
                none,
                None,
            ),
            ButtonVariant::Primary => (
                c.primary,
                c.primary_foreground,
                Some(c.primary_hover),
                None,
                none,
                None,
            ),
            ButtonVariant::Secondary => (
                none,
                theme.content(0.70),
                Some(theme.content(0.10)),
                Some(c.content),
                theme.content(0.10),
                None,
            ),
            ButtonVariant::Ghost if selected => (c.selection, c.content, None, None, none, None),
            ButtonVariant::Ghost => (
                none,
                theme.content(0.50),
                Some(theme.content(0.10)),
                Some(c.content),
                none,
                None,
            ),
            ButtonVariant::Danger => (
                none,
                c.danger,
                Some(crate::color::with_alpha(c.danger, 0.1)),
                None,
                theme.content(0.10),
                Some(crate::color::with_alpha(c.danger, 0.4)),
            ),
        };
        let interactive = !disabled;
        let mut el = div()
            .id(self.id)
            .group("monocode-button")
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .gap(u(6.))
            .px(u(10.))
            .py(u(if self.compact { 2. } else { 4. }))
            .rounded(u(theme.radius.md))
            .text_px(text)
            .leading(theme.leading.label)
            .whitespace_nowrap()
            .bg(fill)
            .text_color(ink)
            .border_1()
            .border_color(border);
        if matches!(self.variant, ButtonVariant::Primary) {
            el = el.medium();
        }
        if self.full_width {
            el = el.w_full();
        }
        if disabled && !matches!(self.variant, ButtonVariant::Primary) {
            el = el.opacity(0.4);
        }
        if interactive {
            el = el.hover(move |s| {
                let mut s = s;
                if let Some(fill) = hover_fill {
                    s = s.bg(fill);
                }
                if let Some(ink) = hover_ink {
                    s = s.text_color(ink);
                }
                if let Some(border) = hover_border {
                    s = s.border_color(border);
                }
                s
            });
        }
        if let Some(name) = self.icon {
            let glyph = icon(name).size(u(14.)).text_color(ink);
            let glyph = match (interactive, hover_ink) {
                (true, Some(hover)) => {
                    glyph.group_hover("monocode-button", move |s| s.text_color(hover))
                }
                _ => glyph,
            };
            el = el.child(glyph);
        }
        el = el.child(self.label);
        if let Some(text) = self.tooltip {
            el = el.tooltip(tooltip(text));
        }
        if let (Some(handler), true) = (self.on_click, interactive) {
            el = el
                .cursor(CursorStyle::Arrow)
                .on_click(move |event, window, cx| handler(event, window, cx));
        }
        el
    }
}

/// A square icon button. TitleBar's `IconButton` is the default: 26px, 14px
/// glyph, `text-content/50` that brightens on hover.
#[derive(IntoElement)]
pub struct IconButton {
    id: ElementId,
    icon: IconName,
    size: f32,
    icon_size: f32,
    active: bool,
    accent: bool,
    disabled: bool,
    tooltip: Option<SharedString>,
    on_click: Option<ClickHandler>,
}

pub fn icon_button(id: impl Into<ElementId>, icon: IconName) -> IconButton {
    IconButton {
        id: id.into(),
        icon,
        size: 26.,
        icon_size: 14.,
        active: false,
        accent: false,
        disabled: false,
        tooltip: None,
        on_click: None,
    }
}

impl IconButton {
    /// Box size in CSS px: 26 (`size-6.5`), 24 (`size-6`), 20 (`size-5`).
    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

    pub fn icon_size(mut self, size: f32) -> Self {
        self.icon_size = size;
        self
    }

    /// Full-strength ink, for a toggled panel.
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    /// Accent ink.
    pub fn accent(mut self, accent: bool) -> Self {
        self.accent = accent;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn tooltip(mut self, text: impl Into<SharedString>) -> Self {
        self.tooltip = Some(text.into());
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

impl RenderOnce for IconButton {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let c = theme.colors;
        let hover_bg = theme.content(0.10);
        let ink = if self.disabled {
            theme.content(0.25)
        } else if self.accent {
            c.accent
        } else if self.active {
            c.content
        } else {
            theme.content(0.50)
        };
        let mut el = div()
            .id(self.id)
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(u(self.size))
            .rounded(u(theme.radius.md))
            .group("monocode-icon-button");
        let glyph = icon(self.icon).size(u(self.icon_size)).text_color(ink);
        if self.disabled {
            el = el.child(glyph);
        } else {
            let hover_ink = if self.accent { c.accent } else { c.content };
            el = el
                .hover(move |s| s.bg(hover_bg))
                .child(glyph.group_hover("monocode-icon-button", move |s| s.text_color(hover_ink)));
        }
        if let Some(text) = self.tooltip {
            el = el.tooltip(tooltip(text));
        }
        if let (Some(handler), false) = (self.on_click, self.disabled) {
            el = el.on_click(move |event, window, cx| handler(event, window, cx));
        }
        el
    }
}
