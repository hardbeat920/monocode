//! The project menu's Theme colors dialog: Appearance's color settings, kept
//! for one project and shown live while it is open.
use crate::{
    popover::AnchorSlot,
    project_settings::{Event, button},
    settings::{ACCENTS, PALETTES, swatch},
    theme::*,
};
use gpui::{
    AnyElement, App, Context, EventEmitter, FocusHandle, Focusable, MouseButton, MouseDownEvent,
    MouseMoveEvent, Role, Window, div, prelude::*, px, rgb,
};
use monocode_core::project_appearance::{ProjectAppearance, ProjectTheme};

#[derive(Clone, Copy, PartialEq)]
enum Slider {
    Hue,
    Saturation,
    DarkLightness,
}
impl Slider {
    const ALL: [Self; 3] = [Self::Hue, Self::Saturation, Self::DarkLightness];
    fn label(self) -> &'static str {
        match self {
            Self::Hue => "Hue",
            Self::Saturation => "Saturation",
            Self::DarkLightness => "Dark-mode lightness",
        }
    }
    fn id(self) -> &'static str {
        match self {
            Self::Hue => "project-theme-hue",
            Self::Saturation => "project-theme-saturation",
            Self::DarkLightness => "project-theme-dark-lightness",
        }
    }
    fn max(self) -> u32 {
        match self {
            Self::Hue => 360,
            Self::Saturation => 100,
            Self::DarkLightness => 30,
        }
    }
    fn get(self, theme: &ProjectTheme) -> u32 {
        match self {
            Self::Hue => theme.hue,
            Self::Saturation => theme.saturation,
            Self::DarkLightness => theme.dark_lightness,
        }
    }
    fn set(self, theme: &mut ProjectTheme, value: u32) {
        match self {
            Self::Hue => theme.hue = value,
            Self::Saturation => theme.saturation = value,
            Self::DarkLightness => theme.dark_lightness = value,
        }
    }
    fn display(self, value: u32) -> String {
        match self {
            Self::Hue => format!("{value}°"),
            _ => format!("{value}%"),
        }
    }
}

pub(crate) struct ThemeDialog {
    project: String,
    appearance: ProjectAppearance,
    /// The app-wide colors, where a project without its own starts.
    app: ProjectTheme,
    light: bool,
    focus: FocusHandle,
    sliders: [FocusHandle; 3],
    dragging: Option<Slider>,
    tracks: [AnchorSlot; 3],
    bounds: AnchorSlot,
}
impl EventEmitter<Event> for ThemeDialog {}
impl Focusable for ThemeDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
impl ThemeDialog {
    pub(crate) fn new(
        project: String,
        appearance: ProjectAppearance,
        app: ProjectTheme,
        light: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            project,
            appearance,
            app,
            light,
            focus: cx.focus_handle(),
            sliders: [cx.focus_handle(), cx.focus_handle(), cx.focus_handle()],
            dragging: None,
            tracks: Default::default(),
            bounds: AnchorSlot::default(),
        }
    }
    pub(crate) fn measured_height(&self) -> Option<f32> {
        self.bounds.get().map(|bounds| bounds.size.height.as_f32())
    }
    fn shown(&self) -> ProjectTheme {
        self.appearance
            .theme
            .clone()
            .unwrap_or_else(|| self.app.clone())
    }
    /// The first change gives the project its own colors.
    fn change(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut ProjectTheme)) {
        let mut theme = self.shown();
        change(&mut theme);
        theme.normalize();
        if self.appearance.theme.as_ref() == Some(&theme) {
            return;
        }
        self.appearance.theme = Some(theme);
        cx.emit(Event::Changed(self.appearance.clone()));
        cx.notify();
    }
    fn use_app_colors(&mut self, cx: &mut Context<Self>) {
        self.appearance.theme = None;
        cx.emit(Event::Changed(self.appearance.clone()));
        cx.notify();
    }
    fn heading(text: &'static str) -> gpui::Div {
        div()
            .text_size(px(13.))
            .line_height(px(19.5))
            .font_weight(gpui::FontWeight::MEDIUM)
            .child(text)
    }
    fn section() -> gpui::Div {
        div()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap_2()
            .pt_4()
            .border_t_1()
            .border_color(content(0.07))
    }
    fn palettes(&self, theme: &ProjectTheme, cx: &mut Context<Self>) -> AnyElement {
        let light = self.light;
        div()
            .debug_selector(|| "project-theme-palettes".into())
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap_2()
            .child(Self::heading("Workspace theme"))
            .child(
                div()
                    .id("project-theme-palette-group")
                    .role(Role::RadioGroup)
                    .aria_label("Workspace theme")
                    .flex()
                    .flex_wrap()
                    .gap(px(6.))
                    .children(
                        PALETTES
                            .iter()
                            .enumerate()
                            .map(|(ix, &(id, label, dark, pale))| {
                                let palette = if light { pale } else { dark };
                                let selected = theme.workspace_theme == id;
                                div()
                                    .id(("project-theme-palette", ix))
                                    .debug_selector(move || format!("project-theme-palette-{id}"))
                                    .role(Role::RadioButton)
                                    .aria_label(label)
                                    .aria_selected(selected)
                                    .h(px(28.))
                                    .px_2()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .rounded_md()
                                    .border_1()
                                    .border_color(content(if selected { 0.35 } else { 0.1 }))
                                    .when(selected, |d| d.bg(content(0.08)))
                                    .text_size(px(12.))
                                    .text_color(content(if selected { 1. } else { 0.7 }))
                                    .cursor_pointer()
                                    .hover(|s| s.bg(content(0.06)).text_color(content(1.)))
                                    .child(swatch(
                                        palette.surface,
                                        palette.panel,
                                        palette.solid,
                                        palette.accent,
                                    ))
                                    .child(label)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        // As Appearance's preset picker: the preset's
                                        // dark surface sets the window tint too.
                                        let surface: gpui::Hsla =
                                            rgb(crate::settings::palette(id, false).surface).into();
                                        this.change(cx, |theme| {
                                            theme.workspace_theme = id.into();
                                            theme.hue = (surface.h * 360.).round() as u32;
                                            theme.saturation = (surface.s * 100.).round() as u32;
                                            theme.dark_lightness =
                                                ((surface.l * 100.).round() as u32).min(30);
                                        });
                                    }))
                            }),
                    ),
            )
            .into_any_element()
    }
    fn accents(&self, theme: &ProjectTheme, cx: &mut Context<Self>) -> AnyElement {
        let current = theme
            .accent_color
            .as_deref()
            .and_then(|hex| u32::from_str_radix(hex.trim_start_matches('#'), 16).ok());
        let choices = std::iter::once((None, "Default"))
            .chain(ACCENTS.iter().map(|&(c, label)| (Some(c), label)));
        Self::section()
            .debug_selector(|| "project-theme-accent".into())
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_4()
                    .child(Self::heading("Accent color"))
                    .child(
                        div()
                            .id("project-theme-accent-group")
                            .role(Role::RadioGroup)
                            .aria_label("Accent color")
                            .flex()
                            .items_center()
                            .gap_1()
                            .children(choices.enumerate().map(|(ix, (color, label))| {
                                let selected = current == color;
                                let fill = color.map_or(content(1.), |c| rgb(c).into());
                                div()
                                    .id(("project-theme-accent", ix))
                                    .debug_selector(move || format!("project-theme-accent-{ix}"))
                                    .role(Role::RadioButton)
                                    .aria_label(label)
                                    .aria_selected(selected)
                                    .size(px(22.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_full()
                                    .cursor_pointer()
                                    .hover(|s| s.bg(content(0.08)))
                                    .child(
                                        div()
                                            .size(px(14.))
                                            .rounded_full()
                                            .bg(fill)
                                            .when(selected, |d| {
                                                d.border_2().border_color(content(0.8))
                                            }),
                                    )
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.change(cx, |theme| {
                                            theme.accent_color = color.map(|c| format!("#{c:06x}"))
                                        })
                                    }))
                            })),
                    ),
            )
            .into_any_element()
    }
    fn slide(&mut self, slider: Slider, x: gpui::Pixels, cx: &mut Context<Self>) {
        let ix = Slider::ALL.iter().position(|s| *s == slider).unwrap_or(0);
        let Some(bounds) = self.tracks[ix].get() else {
            return;
        };
        let travel = (bounds.size.width.as_f32() - 12.).max(1.);
        let fraction = ((x - bounds.left()).as_f32() - 6.).clamp(0., travel) / travel;
        let value = (slider.max() as f32 * fraction).round() as u32;
        self.change(cx, |theme| slider.set(theme, value));
    }
    fn slider(&self, slider: Slider, theme: &ProjectTheme, cx: &mut Context<Self>) -> AnyElement {
        let ix = Slider::ALL.iter().position(|s| *s == slider).unwrap_or(0);
        let value = slider.get(theme);
        let disabled = slider == Slider::DarkLightness && self.light;
        let fraction = value as f32 / slider.max() as f32;
        let track = self.tracks[ix].clone();
        div()
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_between()
            .gap_4()
            .child(Self::heading(slider.label()))
            .child(
                div()
                    .w(px(224.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap_3()
                    .when(disabled, |d| d.opacity(0.4))
                    .child(
                        div()
                            .id(slider.id())
                            .debug_selector(move || slider.id().into())
                            .role(Role::Slider)
                            .aria_label(slider.label())
                            .aria_value(slider.display(value))
                            .track_focus(&self.sliders[ix])
                            .tab_index(0)
                            .relative()
                            .w(px(172.))
                            .h(px(12.))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .child(track.probe())
                            .child(div().w_full().h(px(4.)).rounded_full().bg(content(0.15)))
                            .child(
                                div()
                                    .absolute()
                                    .top_0()
                                    .left(px(fraction * 160.))
                                    .size(px(12.))
                                    .rounded_full()
                                    .bg(content(1.)),
                            )
                            .when(!disabled, |d| {
                                d.on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                                        window.focus(&this.sliders[ix], cx);
                                        this.dragging = Some(slider);
                                        this.slide(slider, event.position.x, cx);
                                        cx.stop_propagation();
                                    }),
                                )
                                .on_mouse_move(cx.listener(
                                    move |this, event: &MouseMoveEvent, _, cx| {
                                        if this.dragging == Some(slider)
                                            && event.pressed_button == Some(MouseButton::Left)
                                        {
                                            this.slide(slider, event.position.x, cx);
                                        }
                                    },
                                ))
                                .on_key_down(cx.listener(
                                    move |this, event: &gpui::KeyDownEvent, _, cx| {
                                        let old = slider.get(&this.shown());
                                        let next = match event.keystroke.key.as_str() {
                                            "left" | "down" => old.saturating_sub(1),
                                            "right" | "up" => old + 1,
                                            "home" => 0,
                                            "end" => slider.max(),
                                            _ => return,
                                        };
                                        this.change(cx, |theme| {
                                            slider.set(theme, next.min(slider.max()))
                                        });
                                        cx.stop_propagation();
                                    },
                                ))
                            }),
                    )
                    .child(
                        div()
                            .w(px(40.))
                            .text_right()
                            .text_size(px(12.))
                            .line_height(px(18.))
                            .font_features(crate::right_panel::tabular())
                            .child(slider.display(value)),
                    ),
            )
            .into_any_element()
    }
}
impl Render for ThemeDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.shown();
        let custom = self.appearance.theme.is_some();
        let label = self.appearance.label(&self.project);
        div()
            .id("project-theme-dialog")
            .debug_selector(|| "project-theme-dialog".into())
            .role(Role::Dialog)
            .aria_label("Theme Colors")
            .track_focus(&self.focus)
            .relative()
            .min_h_0()
            .max_h(px(window.viewport_size().height.as_f32() - 34.))
            .flex()
            .flex_col()
            .text_color(content(1.))
            .child(self.bounds.layout_probe())
            .on_key_down(cx.listener(|_, event: &gpui::KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    cx.emit(Event::Close);
                    window.prevent_default();
                    cx.stop_propagation();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.dragging = None),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .flex()
                    .items_start()
                    .gap_2()
                    .px_4()
                    .pt_3()
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .pt(px(2.))
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .line_height(px(20.))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child("Theme Colors"),
                            )
                            .child(
                                div()
                                    .mt(px(2.))
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(12.))
                                    .line_height(px(16.5))
                                    .text_color(content(0.5))
                                    .child(if custom {
                                        format!("Used while {label} is open")
                                    } else {
                                        format!("{label} uses the app colors")
                                    }),
                            ),
                    )
                    .child(
                        icon_button("close-project-theme", "Close theme colors", "close")
                            .size(px(28.))
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(Event::Close))),
                    ),
            )
            .child(
                div()
                    .id("project-theme-body")
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .p_4()
                    .child(self.palettes(&theme, cx))
                    .child(self.accents(&theme, cx))
                    .child(
                        Self::section()
                            .gap_3()
                            .children(Slider::ALL.map(|slider| self.slider(slider, &theme, cx))),
                    )
                    .child(
                        Self::section()
                            .child(
                                button("project-theme-use-app-colors", "Use app colors")
                                    .flex_shrink_0()
                                    .when(!custom, |d| d.opacity(0.4))
                                    .when(custom, |d| {
                                        d.on_click(
                                            cx.listener(|this, _, _, cx| this.use_app_colors(cx)),
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .line_height(px(17.875))
                                    .text_color(content(0.45))
                                    .child(
                                        "Light or dark mode, translucency and diff colors follow Settings › Appearance.",
                                    ),
                            ),
                    ),
            )
    }
}
