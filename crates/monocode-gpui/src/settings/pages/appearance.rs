//! AppearancePage, ChatBackgroundCard, workspace themes and AccentColorPicker.
use crate::popover::{Align, Anchor, Side, popover};
use crate::settings::{
    SettingsEvent, SettingsView, SliderKind, preferences::UI_SCALE_PERCENTS,
    widgets::secondary_button,
};
use crate::theme::*;
use gpui::{
    AnyElement, Context, Focusable, Role, SharedString, Window, div, linear_color_stop,
    linear_gradient, prelude::*, px, rgb,
};

/// ACCENT_COLOR_PRESETS, after the Default swatch.
const ACCENTS: [(u32, &str); 6] = [
    (0x4da3f5, "Blue"),
    (0x8b5cf6, "Violet"),
    (0xec4899, "Pink"),
    (0xef4444, "Red"),
    (0xf59e0b, "Orange"),
    (0x10b981, "Green"),
];

const EFFECTS: [(&str, &str); 6] = [
    ("none", "None"),
    ("dither", "Dither"),
    ("ascii", "ASCII"),
    ("halftone", "Halftone"),
    ("scanlines", "Scanlines"),
    ("gradient-blur", "Haze"),
];
fn effect_description(effect: &str) -> &'static str {
    match effect {
        "dither" => "Rebuilds the artwork with a dithered color palette.",
        "ascii" => "Recreates the artwork with colored characters on black.",
        "halftone" => "Recreates the artwork with colored print dots on black.",
        "scanlines" => "Adds a pronounced horizontal display-line texture.",
        "gradient-blur" => "Blurs and fades the artwork into the background below.",
        _ => "Shows the original artwork.",
    }
}

pub(crate) fn parse_hex(text: &str) -> Option<u32> {
    let hex = text.strip_prefix('#')?;
    (hex.len() == 6)
        .then(|| u32::from_str_radix(hex, 16).ok())
        .flatten()
}

/// Preview the palette's neutral tones and accent at a menu icon's 16px.
fn swatch(surface: u32, panel: u32, solid: u32, accent: u32) -> AnyElement {
    div()
        .relative()
        .size_4()
        .overflow_hidden()
        .rounded(px(4.))
        .border_1()
        .border_color(content(0.15))
        .bg(rgb(surface))
        .child(
            div()
                .absolute()
                .top(px(2.))
                .bottom(px(2.))
                .left(px(4.))
                .right(px(-1.))
                .rounded_l(px(2.))
                .border_1()
                .border_color(rgb(solid))
                .bg(rgb(panel)),
        )
        .child(
            div()
                .absolute()
                .top(px(5.))
                .left(px(7.))
                .size(px(5.))
                .rounded_full()
                .bg(rgb(accent)),
        )
        .into_any_element()
}

impl SettingsView {
    pub(super) fn appearance_page(
        &mut self,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let p = self.prefs.clone();
        let inset = !self.ctx.classic;
        let light = p.theme == "light"
            || (p.theme == "system"
                && matches!(
                    cx.window_appearance(),
                    gpui::WindowAppearance::Light | gpui::WindowAppearance::VibrantLight
                ));
        let mut theme = vec![self.row(
            Some("theme"),
            true,
            "Theme",
            Some("System follows the OS appearance.".into()),
            vec![self.segmented(
                "theme",
                "Theme",
                &p.theme,
                &[("system", "System"), ("dark", "Dark"), ("light", "Light")],
                cx,
                |this, v, cx| this.update(cx, |p| p.theme = v.into()),
            )],
        )];
        let options = crate::settings::palettes::PALETTES
            .iter()
            .map(|&(id, label, dark, pale)| {
                let palette = if light { pale } else { dark };
                (
                    id.to_owned(),
                    SharedString::from(label),
                    Some(swatch(
                        palette.surface,
                        palette.panel,
                        palette.solid,
                        palette.accent,
                    )),
                )
            })
            .collect();
        theme.push(self.row(
            Some("workspace-theme"),
            false,
            "Workspace theme",
            Some("Choose a color preset.".into()),
            vec![self.select(
                "workspace-theme",
                "Workspace theme",
                &p.workspace_theme,
                options,
                Some(192.),
                cx,
                |this, v, cx| this.update(cx, |p| p.apply_workspace_theme(v)),
            )],
        ));
        #[cfg(target_os = "linux")]
        if cx.compositor_name() == "Wayland" {
            theme.push(self.row(
                Some("wayland-window-decorations"),
                false,
                "Wayland window decorations",
                Some("Show compositor decorations. Monocode's title bar stays visible.".into()),
                vec![self.toggle(
                    "wayland-window-decorations",
                    "Wayland window decorations",
                    p.wayland_window_decorations,
                    false,
                    cx,
                    |this, on, cx| this.update(cx, |p| p.wayland_window_decorations = on),
                )],
            ));
        }
        let accent_picker = self.accent_picker(cx);
        theme.push(self.row(
            Some("accent-color"),
            false,
            "Accent color",
            Some("Used for the composer send button and your message bubbles.".into()),
            vec![accent_picker],
        ));
        theme.push(self.row(
            Some("diff-colors"),
            false,
            "Diff colors",
            Some("Colorblind and High contrast use blue and orange for changes.".into()),
            vec![self.segmented(
                "diff-colors",
                "Diff colors",
                &p.diff_palette,
                &[
                    ("default", "Default"),
                    ("colorblind", "Colorblind"),
                    ("high-contrast", "High contrast"),
                ],
                cx,
                |this, v, cx| this.update(cx, |p| p.diff_palette = v.into()),
            )],
        ));
        let mut page = vec![self.group(None, true, "Theme", None, None, theme)];
        let color = vec![
                self.row(
                    Some("hue"),
                    true,
                    "Hue",
                    Some("Base hue for the window and main-pane surfaces.".into()),
                    vec![self.slider(SliderKind::Hue, format!("{}°", p.theme_hue), false, cx)],
                ),
                self.row(
                    Some("saturation"),
                    false,
                    "Saturation",
                    Some("How strongly the hue tints the interface. Zero keeps it neutral.".into()),
                    vec![self.slider(
                        SliderKind::Saturation,
                        format!("{}%", p.theme_saturation),
                        false,
                        cx,
                    )],
                ),
                self.row(
                    Some("dark-lightness"),
                    false,
                    "Dark-mode lightness",
                    Some(if light {
                        "This only affects dark mode. Your dark-mode value is preserved.".into()
                    } else {
                        "Base brightness of the dark theme. Lower values are darker; zero is true black.".into()
                    }),
                    vec![self.slider(
                        SliderKind::DarkLightness,
                        format!("{}%", p.theme_dark_lightness),
                        light,
                        cx,
                    )],
                ),
            ];
        page.push(self.group(None, false, "Color", None, None, color));
        let translucency = vec![
                self.row(
                    Some("sidebar-opacity"),
                    true,
                    "Sidebar opacity",
                    Some("Applies to the project rail and the other glass panes.".into()),
                    vec![self.slider(
                        SliderKind::SidebarOpacity,
                        format!("{}%", p.sidebar_opacity),
                        light,
                        cx,
                    )],
                ),
                self.row(
                    Some("main-pane-glass"),
                    false,
                    "Main pane glass",
                    Some("Extend the translucent treatment to the main pane behind sessions and editors.".into()),
                    vec![self.toggle(
                        "main-pane-glass",
                        "Main pane glass",
                        p.body_glass,
                        light,
                        cx,
                        |this, on, cx| this.update(cx, |p| p.body_glass = on),
                    )],
                ),
            ];
        page.push(self.group(
            None,
            false,
            "Translucency",
            Some(if light {
                "Translucency is available in dark mode.".into()
            } else {
                "How much of the desktop shows through MonoCode.".into()
            }),
            None,
            translucency,
        ));
        page.push(self.chat_background_card(cx));
        let scale_options = UI_SCALE_PERCENTS
            .iter()
            .map(|percent| {
                (
                    percent.to_string(),
                    SharedString::from(format!("{percent}%")),
                    None,
                )
            })
            .collect();
        let mut layout = vec![
            self.row(
                Some("inset-layout"),
                true,
                "Layout",
                Some("Classic keeps everything in one sidebar. Focused adds a project switcher, workspace tabs and a right panel.".into()),
                vec![self.segmented(
                    "inset-layout",
                    "Layout",
                    if inset { "focused" } else { "classic" },
                    &[("classic", "Classic"), ("focused", "Focused")],
                    cx,
                    |_, value, cx| cx.emit(SettingsEvent::NewLayout(value == "focused")),
                )],
            ),
            self.row(
                Some("interface-scale"),
                false,
                "Interface scale",
                Some("Zoom the whole interface. You can also use Ctrl+=, Ctrl+-, and Ctrl+0 (Cmd on macOS).".into()),
                vec![self.select(
                    "interface-scale",
                    "Interface scale",
                    &p.ui_scale.to_string(),
                    scale_options,
                    None,
                    cx,
                    |this, v, cx| {
                        if let Ok(v) = v.parse() {
                            this.update(cx, |p| p.ui_scale = v)
                        }
                    },
                )],
            ),
            self.row(
                Some("show-excluded-files"),
                false,
                "Show excluded files",
                Some("Show files and folders Git excludes, such as build output and dependencies, in the explorer.".into()),
                vec![self.toggle(
                    "show-excluded-files",
                    "Show excluded files",
                    p.show_excluded_files,
                    false,
                    cx,
                    |this, on, cx| this.update(cx, |p| p.show_excluded_files = on),
                )],
            ),
        ];
        if inset {
            layout.insert(
                1,
                self.row(
                    Some("new-layout-style"),
                    false,
                    "Workspace style",
                    Some(if cfg!(target_os = "macos") {
                        "Choose a rounded card or a full-height boxy workspace. ⌘⌥L switches between them.".into()
                    } else {
                        "Choose a rounded card or a full-height boxy workspace. Ctrl+Alt+L switches between them.".into()
                    }),
                    vec![self.segmented(
                        "new-layout-style",
                        "Workspace style",
                        if self.ctx.new_layout_boxy {
                            "boxy"
                        } else {
                            "rounded"
                        },
                        &[("rounded", "Rounded"), ("boxy", "Boxy")],
                        cx,
                        |_, value, cx| cx.emit(SettingsEvent::NewLayoutBoxy(value == "boxy")),
                    )],
                ),
            );
        }
        page.push(self.group(None, false, "Layout", None, None, layout));
        page
    }

    /// AccentColorPicker: Default plus six presets and a custom color.
    fn accent_picker(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let value = self.prefs.accent_color.clone();
        let preset = match &value {
            None => Some(0),
            Some(hex) => ACCENTS
                .iter()
                .position(|(c, _)| Some(*c) == parse_hex(hex))
                .map(|ix| ix + 1),
        };
        let custom = preset
            .is_none()
            .then(|| value.as_deref().and_then(parse_hex))
            .flatten();
        let weak = cx.weak_entity();
        let dot = |selected: bool, color: gpui::Hsla| {
            div()
                .size(px(14.))
                .rounded_full()
                .bg(color)
                .when(selected, |d| d.border_2().border_color(content(0.8)))
        };
        let colors: Vec<(gpui::Hsla, &'static str, Option<String>)> =
            std::iter::once((content(1.), "Default", None))
                .chain(
                    ACCENTS
                        .iter()
                        .map(|&(c, label)| (rgb(c).into(), label, Some(format!("#{c:06x}")))),
                )
                .collect();
        div()
            .w(px(192.))
            .relative()
            .child(self.accent_anchor.probe())
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_1()
                    .px(px(2.))
                    .children(
                        colors
                            .into_iter()
                            .enumerate()
                            .map(|(ix, (color, label, hex))| {
                                div()
                                    .id(("accent-color", ix))
                                    .debug_selector(move || format!("accent-color-{ix}"))
                                    .role(Role::Button)
                                    .aria_label(label)
                                    .aria_selected(preset == Some(ix))
                                    .size(px(20.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_full()
                                    .cursor_pointer()
                                    .child(dot(preset == Some(ix), color))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.accent_custom_open = false;
                                        let hex = hex.clone();
                                        this.update(cx, |p| p.accent_color = hex);
                                    }))
                            }),
                    )
                    .child(
                        div()
                            .id("accent-custom")
                            .role(Role::Button)
                            .aria_label("Custom color")
                            .aria_expanded(self.accent_custom_open)
                            .size(px(20.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .cursor_pointer()
                            .child(
                                div()
                                    .size(px(14.))
                                    .rounded_full()
                                    .overflow_hidden()
                                    .when(custom.is_some() || self.accent_custom_open, |d| {
                                        d.border_2().border_color(content(0.8))
                                    })
                                    .map(|d| match custom {
                                        Some(c) => d.bg(rgb(c)),
                                        None => d.bg(linear_gradient(
                                            135.,
                                            linear_color_stop(rgb(0xff4d4d), 0.),
                                            linear_color_stop(rgb(0x4d6bff), 1.),
                                        )),
                                    }),
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.accent_custom_open = !this.accent_custom_open;
                                if this.accent_custom_open {
                                    let text = this
                                        .prefs
                                        .accent_color
                                        .clone()
                                        .unwrap_or_else(|| "#4da3f5".into());
                                    this.accent_custom.update(cx, |i, cx| {
                                        i.set_text(text, cx);
                                        i.select_range(0..i.content.len(), cx);
                                    });
                                    window.focus(&this.accent_custom.focus_handle(cx), cx);
                                }
                                cx.notify();
                            })),
                    ),
            )
            .when(self.accent_custom_open, |d| {
                d.child(
                    popover(
                        "accent-custom-popover",
                        Anchor::Slot(self.accent_anchor.clone()),
                    )
                    .side(Side::Bottom)
                    .align(Align::End)
                    .width(248.)
                    .padding(8.)
                    .on_dismiss(move |_, cx| {
                        let _ = weak.update(cx, |this, cx| {
                            this.accent_custom_open = false;
                            cx.notify();
                        });
                    })
                    .child(
                        div()
                            .h(px(30.))
                            .px_2()
                            .flex()
                            .items_center()
                            .gap_2()
                            .rounded_md()
                            .border_1()
                            .border_color(content(0.1))
                            .child(
                                div()
                                    .size(px(14.))
                                    .flex_shrink_0()
                                    .rounded_full()
                                    .bg(rgb(custom.unwrap_or(0x4da3f5))),
                            )
                            .child(div().flex_1().min_w_0().child(self.accent_custom.clone())),
                    ),
                )
            })
            .into_any_element()
    }

    /// ChatBackgroundCard: the global image behind every chat pane.
    fn chat_background_card(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = self.prefs.clone();
        let image = p
            .chat_background
            .clone()
            .filter(|path| std::path::Path::new(path).is_file());
        let empty = p.chat_background_empty;
        let session = p.chat_background_session;
        let preview = div()
            .p_4()
            .child(
                div()
                    .overflow_hidden()
                    .rounded_lg()
                    .border_1()
                    .border_color(content(0.1))
                    .map(|d| match &image {
                        Some(path) => d.child(
                            div()
                                .relative()
                                .h(px(144.))
                                .child(
                                    crate::image_memory::preview(std::path::PathBuf::from(path))
                                        .size_full()
                                        .object_fit(gpui::ObjectFit::Cover)
                                        .opacity(empty as f32 / 100.),
                                )
                                .child(
                                    div()
                                        .absolute()
                                        .bottom_2()
                                        .left_2()
                                        .text_size(px(11.))
                                        .text_color(content(0.4))
                                        .child(format!("Empty chat preview at {empty}%")),
                                ),
                        ),
                        None => d.child(
                            div()
                                .id("choose-chat-background")
                                .role(Role::Button)
                                .aria_label("Choose an image")
                                .h(px(144.))
                                .w_full()
                                .flex()
                                .flex_col()
                                .items_center()
                                .justify_center()
                                .gap_2()
                                .text_color(content(0.4))
                                .cursor_pointer()
                                .hover(|s| s.bg(content(0.05)).text_color(content(0.7)))
                                .child(icon("image-add", 20.).text_color(content(0.4)))
                                .child(div().text_size(px(12.)).child("Choose an image"))
                                .on_click(cx.listener(|_, _, _, cx| {
                                    cx.emit(SettingsEvent::ChooseBackground)
                                })),
                        ),
                    }),
            )
            .when(image.is_some(), |d| {
                d.child(
                    div()
                        .mt_3()
                        .flex()
                        .items_center()
                        .justify_end()
                        .gap_2()
                        .child(
                            secondary_button("change-chat-background", "Change", false, false)
                                .child("Change")
                                .on_click(cx.listener(|_, _, _, cx| {
                                    cx.emit(SettingsEvent::ChooseBackground)
                                })),
                        )
                        .child(
                            secondary_button("remove-chat-background", "Remove", true, false)
                                .child("Remove")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.update(cx, |p| p.chat_background = None)
                                })),
                        ),
                )
            })
            .into_any_element();
        let mut rows = vec![preview];
        if image.is_some() {
            rows.push(self.row(
                None,
                false,
                "Background effect",
                Some(effect_description(&p.chat_background_effect).into()),
                vec![self.segmented(
                    "background-effect",
                    "Background effect",
                    &p.chat_background_effect,
                    &EFFECTS,
                    cx,
                    |this, v, cx| this.update(cx, |p| p.chat_background_effect = v.into()),
                )],
            ));
            rows.push(self.row(
                None,
                false,
                "Show on",
                Some("Empty sessions only, or every conversation.".into()),
                vec![self.segmented(
                    "background-scope",
                    "Show background on",
                    &p.chat_background_scope,
                    &[("empty", "Empty only"), ("all", "All sessions")],
                    cx,
                    |this, v, cx| this.update(cx, |p| p.chat_background_scope = v.into()),
                )],
            ));
            rows.push(self.row(
                None,
                false,
                "Empty chat visibility",
                Some("Background strength before a chat has messages.".into()),
                vec![self.slider(SliderKind::BackgroundEmpty, format!("{empty}%"), false, cx)],
            ));
            rows.push(self.row(
                None,
                false,
                "Session visibility",
                Some("Background strength once the conversation has messages.".into()),
                vec![self.slider(
                    SliderKind::BackgroundSession,
                    format!("{session}%"),
                    false,
                    cx,
                )],
            ));
        }
        self.group(
            Some("chat-background"),
            false,
            "Chat background",
            Some("An image behind your chat panes. It stays on this device.".into()),
            None,
            rows,
        )
    }
}
