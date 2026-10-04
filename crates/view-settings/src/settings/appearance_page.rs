//! Port of `AppearancePage` and `ChatBackgroundCard` in SettingsView.tsx.

use gpui::{
    AnyElement, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement, ObjectFit,
    ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _,
    StyledImage as _, Subscription, Window, div, img, prelude::FluentBuilder as _,
};
use monocode_core::appearance::*;
use monocode_core::js;
use monocode_core::settings::CollapsedProjectRailMode;
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

use super::appearance_state::AppearanceState;
use super::background::{BackgroundImage, ChatBackground, HazeVariant, gradient_blur_background};
use super::chrome::{group, row};
use super::color_picker::AccentColorPicker;
use super::controls::{Leading, secondary_button, segmented, slider, spinner_icon, toggle};
use super::section::SectionContext;
use super::select::{Select, SelectOption};

/// `UI_SCALE_PERCENTS` as select options.
fn ui_scale_options() -> Vec<SelectOption> {
    ui_scale_percents()
        .into_iter()
        .map(|percent| SelectOption::new(percent.to_string(), format!("{percent}%")))
        .collect()
}

fn ui_scale_value(scale: f64) -> String {
    (js::round(scale * 100.0) as i64).to_string()
}

pub struct AppearanceSection {
    ctx: SectionContext,
    appearance: Entity<AppearanceState>,
    accent: Entity<AccentColorPicker>,
    ui_scale: Entity<Select>,
    _subscriptions: Vec<Subscription>,
}

impl AppearanceSection {
    pub fn new(
        ctx: SectionContext,
        appearance: Entity<AppearanceState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let settings = appearance.read(cx).settings.clone();
        // The preview shows the global background; draw it if the app has
        // not yet (`initAppearance` applied it at launch).
        let applied = ChatBackground::global(cx).and_then(|background| background.path.clone());
        if applied.as_deref() != settings.chat_background_path.as_deref() {
            let light = Theme::of(cx).scheme == monocode_ui::ColorScheme::Light;
            ChatBackground::apply_chat_background(
                settings.chat_background_path.as_deref(),
                settings.new_thread_background_effect,
                light,
                cx,
            );
        }
        let state = appearance.clone();
        let accent = cx.new(|cx| {
            AccentColorPicker::new(
                settings.accent_color.clone(),
                move |value, _, cx| {
                    state.update(cx, |state, cx| state.on_accent_color(value, cx));
                },
                cx,
            )
        });
        let state = appearance.clone();
        let ui_scale = cx.new(|cx| {
            Select::new(
                "Interface scale",
                ui_scale_value(settings.ui_scale),
                ui_scale_options(),
                cx,
            )
            .on_change(move |value, _, cx| {
                let percent = value.parse::<f64>().unwrap_or(100.0);
                state.update(cx, |state, cx| state.on_ui_scale(percent, cx));
            })
        });
        let subscriptions = vec![
            cx.observe_in(&appearance, window, |this, appearance, window, cx| {
                let settings = appearance.read(cx).settings.clone();
                this.ui_scale.update(cx, |select, cx| {
                    select.set_value(ui_scale_value(settings.ui_scale), cx)
                });
                this.accent.update(cx, |accent, cx| {
                    accent.set_value(settings.accent_color.clone(), window, cx)
                });
                cx.notify();
            }),
            cx.observe_global::<ChatBackground>(|_, cx| cx.notify()),
        ];
        Self {
            ctx,
            appearance,
            accent,
            ui_scale,
            _subscriptions: subscriptions,
        }
    }

    pub fn ui_scale_select(&self) -> &Entity<Select> {
        &self.ui_scale
    }

    pub fn accent_picker(&self) -> &Entity<AccentColorPicker> {
        &self.accent
    }

    fn update(
        &self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut AppearanceState, &mut Context<AppearanceState>),
    ) {
        self.appearance.update(cx, f);
    }

    /// The preview image: the prepared global background, else the file.
    fn preview_image(path: &str, cx: &Context<Self>) -> BackgroundImage {
        ChatBackground::global(cx)
            .filter(|background| background.path.as_deref() == Some(path))
            .and_then(|background| background.image.clone())
            .unwrap_or_else(|| BackgroundImage::Original(SharedString::from(path.to_string())))
    }

    fn chat_background_card(
        &self,
        settings: &AppearanceSettings,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let reveal = self.ctx.reveal(cx);
        let state = self.appearance.read(cx);
        let busy = state.chat_background_busy;
        let error = state.chat_background_error.clone();
        let path = settings.chat_background_path.clone();
        let empty_visibility = js::round(settings.chat_background_empty_opacity * 100.0) as i64;
        let session_visibility = js::round(settings.chat_background_session_opacity * 100.0) as i64;
        let effect = settings.new_thread_background_effect;

        let preview: AnyElement = match &path {
            Some(path) => {
                let haze = effect == NewThreadBackgroundEffect::GradientBlur;
                let opacity = settings.chat_background_empty_opacity as f32;
                let picture: AnyElement = if haze {
                    gradient_blur_background(
                        "chat-background-preview",
                        path.clone(),
                        ChatBackground::image_revision(cx),
                        HazeVariant::Preview,
                    )
                    .opacity(opacity)
                    .into_any_element()
                } else {
                    let image = Self::preview_image(path, cx);
                    div()
                        .size_full()
                        .opacity(opacity)
                        .debug_selector(|| "chat-background-image".into())
                        .child(
                            img(image.source())
                                .size_full()
                                .rounded(u(theme.radius.lg - 1.))
                                .object_fit(ObjectFit::Cover),
                        )
                        .into_any_element()
                };
                div()
                    .relative()
                    .h(u(144.))
                    .when(haze, |el| el.bg(theme.colors.background_base))
                    .child(picture)
                    .child(
                        div()
                            .absolute()
                            .bottom(u(8.))
                            .left(u(8.))
                            .text_px(theme.text.caption)
                            .text_color(theme.content(0.40))
                            .child(format!("Empty chat preview at {empty_visibility}%")),
                    )
                    .into_any_element()
            }
            None => {
                let hover_fill = theme.content(0.05);
                let hover_ink = theme.content(0.70);
                let mut choose = div()
                    .id("choose-chat-background")
                    .flex()
                    .flex_col()
                    .h(u(144.))
                    .w_full()
                    .items_center()
                    .justify_center()
                    .gap(u(8.))
                    .text_color(theme.content(0.40))
                    .debug_selector(|| "button:choose-chat-background".into())
                    .child(if busy {
                        spinner_icon("chat-background-busy", 20., theme.content(0.40))
                    } else {
                        icon(IconName::ImagePlus)
                            .size(u(20.))
                            .text_color(theme.content(0.40))
                            .into_any_element()
                    })
                    .child(div().text_px(theme.text.label).child("Choose an image"));
                if busy {
                    choose = choose.opacity(0.4);
                } else {
                    choose = choose
                        .hover(move |s| s.bg(hover_fill).text_color(hover_ink))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.update(cx, |state, cx| state.on_choose_chat_background(cx))
                        }));
                }
                choose.into_any_element()
            }
        };

        let mut top = div()
            .p(u(16.))
            .border_b_1()
            .border_color(theme.content(0.05))
            .child(
                div()
                    .overflow_hidden()
                    .rounded(u(theme.radius.lg))
                    .border_1()
                    .border_color(theme.content(0.10))
                    .child(preview),
            );
        if path.is_some() {
            let mut change = secondary_button("change-chat-background", "Change")
                .disabled(busy)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.update(cx, |state, cx| state.on_choose_chat_background(cx))
                }));
            if busy {
                change = change.leading(Leading::Spinner);
            }
            top = top.child(
                div()
                    .mt(u(12.))
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(u(8.))
                    .child(change)
                    .child(
                        secondary_button("remove-chat-background", "Remove")
                            .danger(true)
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.update(cx, |state, cx| state.on_clear_chat_background(cx))
                            })),
                    ),
            );
        }
        if let Some(error) = error {
            top = top.child(
                div()
                    .mt(u(8.))
                    .text_px(theme.text.label)
                    .text_color(theme.colors.danger)
                    .child(error),
            );
        }

        let mut card = group(&reveal, "Chat background")
            .id("chat-background")
            .description("An image behind your chat panes. It stays on this device.")
            .child(top);
        if path.is_some() {
            let min = js::round(CHAT_BACKGROUND_OPACITY_MIN * 100.0);
            let max = js::round(CHAT_BACKGROUND_OPACITY_MAX * 100.0);
            card = card
                .child(
                    row(&reveal, "Background effect")
                        .selector("row:Background effect")
                        .description(effect.description())
                        .child(
                            segmented(
                                "Background effect",
                                effect.as_str(),
                                NewThreadBackgroundEffect::ALL
                                    .iter()
                                    .map(|effect| (effect.as_str(), effect.label())),
                            )
                            .option_id_prefix("new-thread-background-effect")
                            .on_change(cx.listener(|this, value: &str, _, cx| {
                                let next = NewThreadBackgroundEffect::parse(Some(value));
                                this.update(cx, |state, cx| {
                                    state.on_new_thread_background_effect(next, cx)
                                })
                            })),
                        ),
                )
                .child(
                    row(&reveal, "Show on")
                        .selector("row:Show on")
                        .description("Empty sessions only, or every conversation.")
                        .child(
                            segmented(
                                "Show background on",
                                settings.chat_background_scope.as_str(),
                                [("empty", "Empty only"), ("all", "All sessions")],
                            )
                            .on_change(cx.listener(|this, value: &str, _, cx| {
                                let next = ChatBackgroundScope::parse(Some(value));
                                this.update(cx, |state, cx| state.on_chat_background_scope(next, cx))
                            })),
                        ),
                )
                .child(
                    row(&reveal, "Empty chat visibility")
                        .description("Background strength before a chat has messages.")
                        .child(
                            slider(
                                "Empty chat background visibility",
                                empty_visibility as f64,
                                format!("{empty_visibility}%"),
                                min,
                                max,
                            )
                            .on_change(cx.listener(|this, value: &f64, _, cx| {
                                let value = *value;
                                this.update(cx, |state, cx| {
                                    state.on_chat_background_empty_opacity(value, cx)
                                })
                            })),
                        ),
                )
                .child(
                    row(&reveal, "Session visibility")
                        .description("Background strength once the conversation has messages.")
                        .child(
                            slider(
                                "Session background visibility",
                                session_visibility as f64,
                                format!("{session_visibility}%"),
                                min,
                                max,
                            )
                            .on_change(cx.listener(|this, value: &f64, _, cx| {
                                let value = *value;
                                this.update(cx, |state, cx| {
                                    state.on_chat_background_session_opacity(value, cx)
                                })
                            })),
                        ),
                )
                .child(
                    row(&reveal, "Background blur")
                        .description("Blurs the image behind chat panes, including project images. Haze keeps its own blur.")
                        .child(
                            slider(
                                "Chat background blur",
                                settings.chat_background_blur as f64,
                                format!("{}px", settings.chat_background_blur),
                                CHAT_BACKGROUND_BLUR_MIN,
                                CHAT_BACKGROUND_BLUR_MAX,
                            )
                            .on_change(cx.listener(|this, value: &f64, _, cx| {
                                let value = *value;
                                this.update(cx, |state, cx| {
                                    state.on_chat_background_blur(value, cx)
                                })
                            })),
                        ),
                );
        }
        card.into_any_element()
    }
}

impl Render for AppearanceSection {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let reveal = self.ctx.reveal(cx);
        let state = self.appearance.read(cx);
        let settings = state.settings.clone();
        let rail_mode = state.collapsed_project_rail_mode();
        let glass_disabled = Theme::of(cx).scheme == monocode_ui::ColorScheme::Light;
        let percent = js::round(settings.sidebar_opacity * 100.0) as i64;
        let main_percent = js::round(settings.main_opacity * 100.0) as i64;

        let theme_group = group(&reveal, "Theme")
            .first(true)
            .description(
                "Dark and light share the same tint, so the color settings below apply to both.",
            )
            .child(
                row(&reveal, "Theme")
                    .id("theme")
                    .description("System follows the OS appearance.")
                    .child(
                        segmented(
                            "Theme",
                            settings.theme_preference.as_str(),
                            [("system", "System"), ("dark", "Dark"), ("light", "Light")],
                        )
                        .on_change(cx.listener(
                            |this, value: &str, _, cx| {
                                let next = ThemePreference::parse(Some(value));
                                this.update(cx, |state, cx| state.on_theme_preference(next, cx))
                            },
                        )),
                    ),
            )
            .child(
                row(&reveal, "Accent color")
                    .id("accent-color")
                    .description("Used for the composer send button and your message bubbles.")
                    .child(self.accent.clone()),
            )
            .child(
                row(&reveal, "Diff colors")
                    .id("diff-colors")
                    .description(
                        "Colors for added and removed lines. Colorblind and High contrast use blue and orange instead of green and red; High contrast adds stronger tints and text.",
                    )
                    .child(
                        segmented(
                            "Diff colors",
                            settings.diff_palette.as_str(),
                            [
                                ("default", "Default"),
                                ("colorblind", "Colorblind"),
                                ("high-contrast", "High contrast"),
                            ],
                        )
                        .on_change(cx.listener(|this, value: &str, _, cx| {
                            let next = DiffPalette::parse(Some(value));
                            this.update(cx, |state, cx| state.on_diff_palette(next, cx))
                        })),
                    ),
            );

        let hue = settings.theme_hue;
        let saturation = settings.theme_saturation;
        let color_group = group(&reveal, "Color")
            .description("Hue and saturation tint every surface. Lightness only moves the dark theme.")
            .child(
                row(&reveal, "Hue")
                    .id("hue")
                    .description("Base hue for accents and tinted surfaces.")
                    .child(
                        slider("Hue", hue as f64, format!("{hue}°"), THEME_HUE_MIN, THEME_HUE_MAX)
                            .on_change(cx.listener(move |this, value: &f64, _, cx| {
                                let value = *value;
                                this.update(cx, |state, cx| {
                                    let saturation = state.settings.theme_saturation as f64;
                                    state.on_tint(value, saturation, cx)
                                })
                            })),
                    ),
            )
            .child(
                row(&reveal, "Saturation")
                    .id("saturation")
                    .description("How strongly the hue tints the interface. Zero keeps it neutral.")
                    .child(
                        slider(
                            "Saturation",
                            saturation as f64,
                            format!("{saturation}%"),
                            THEME_SATURATION_MIN,
                            THEME_SATURATION_MAX,
                        )
                        .on_change(cx.listener(|this, value: &f64, _, cx| {
                            let value = *value;
                            this.update(cx, |state, cx| {
                                let hue = state.settings.theme_hue as f64;
                                state.on_tint(hue, value, cx)
                            })
                        })),
                    ),
            )
            .child(
                row(&reveal, "Dark-mode lightness")
                    .id("dark-lightness")
                    .description(if glass_disabled {
                        "This only affects dark mode. Your dark-mode value is preserved."
                    } else {
                        "Base brightness of the dark theme. Lower values are darker; zero is true black."
                    })
                    .child(
                        slider(
                            "Dark-mode lightness",
                            settings.theme_dark_lightness as f64,
                            format!("{}%", settings.theme_dark_lightness),
                            THEME_DARK_LIGHTNESS_MIN,
                            THEME_DARK_LIGHTNESS_MAX,
                        )
                        .disabled(glass_disabled)
                        .on_change(cx.listener(|this, value: &f64, _, cx| {
                            let value = *value;
                            this.update(cx, |state, cx| state.on_dark_lightness(value, cx))
                        })),
                    ),
            );

        let translucency = group(&reveal, "Translucency")
            .description(if glass_disabled {
                "Light mode always uses an opaque window, so these are off. Your dark-mode values are preserved."
            } else {
                "How much of the desktop shows through MonoCode. Blur costs more to composite the higher it goes."
            })
            .child(
                row(&reveal, "Sidebar opacity")
                    .id("sidebar-opacity")
                    .description("Applies to the project rail and the session sidebar.")
                    .child(
                        slider(
                            "Sidebar opacity",
                            percent as f64,
                            format!("{percent}%"),
                            js::round(SIDEBAR_OPACITY_MIN * 100.0),
                            js::round(SIDEBAR_OPACITY_MAX * 100.0),
                        )
                        .disabled(glass_disabled)
                        .on_change(cx.listener(|this, value: &f64, _, cx| {
                            let value = *value;
                            this.update(cx, |state, cx| state.on_opacity(value, cx))
                        })),
                    ),
            )
            .child(
                row(&reveal, "Blur radius")
                    .id("blur")
                    .description("Background blur behind the window.")
                    .child(
                        slider(
                            "Blur radius",
                            settings.sidebar_blur as f64,
                            settings.sidebar_blur.to_string(),
                            SIDEBAR_BLUR_MIN,
                            SIDEBAR_BLUR_MAX,
                        )
                        .disabled(glass_disabled)
                        .on_change(cx.listener(|this, value: &f64, _, cx| {
                            let value = *value;
                            this.update(cx, |state, cx| state.on_blur(value, cx))
                        })),
                    ),
            )
            .child(
                row(&reveal, "Main pane glass")
                    .id("main-pane-glass")
                    .description("Extend the translucent treatment to the main pane behind sessions and editors.")
                    .switch_only()
                    .child(
                        toggle("Main pane glass", settings.body_glass)
                            .disabled(glass_disabled)
                            .on_change(cx.listener(|this, next: &bool, _, cx| {
                                let next = *next;
                                this.update(cx, |state, cx| state.on_body_glass(next, cx))
                            })),
                    ),
            )
            .child(
                row(&reveal, "Main pane opacity")
                    .id("main-pane-opacity")
                    .description("Applies to the main pane when main pane glass is on.")
                    .child(
                        slider(
                            "Main pane opacity",
                            main_percent as f64,
                            format!("{main_percent}%"),
                            js::round(MAIN_OPACITY_MIN * 100.0),
                            js::round(MAIN_OPACITY_MAX * 100.0),
                        )
                        .disabled(glass_disabled || !settings.body_glass)
                        .on_change(cx.listener(|this, value: &f64, _, cx| {
                            let value = *value;
                            this.update(cx, |state, cx| state.on_main_opacity(value, cx))
                        })),
                    ),
            );

        let chat_background = self.chat_background_card(&settings, cx);

        let layout = group(&reveal, "Layout")
            .child(
                row(&reveal, "Collapsed project rail")
                    .id("collapsed-project-rail")
                    .description("Keep project navigation available as a compact icon rail, or hide the rail completely.")
                    .child(
                        segmented(
                            "Collapsed project rail",
                            rail_mode.as_str(),
                            [("compact", "Icon rail"), ("hidden", "Hidden")],
                        )
                        .on_change(cx.listener(|this, value: &str, window, cx| {
                            let next = CollapsedProjectRailMode::parse(Some(value));
                            this.appearance.update(cx, |state, cx| {
                                state.on_collapsed_project_rail_mode(next, window, cx)
                            })
                        })),
                    ),
            )
            .child(
                row(&reveal, "Interface scale")
                    .id("interface-scale")
                    .description("Zoom the whole interface. You can also use Ctrl+=, Ctrl+-, and Ctrl+0 (Cmd on macOS).")
                    .child(self.ui_scale.clone()),
            )
            .child(
                row(&reveal, "Show excluded files")
                    .id("show-excluded-files")
                    .description("Show files and folders Git excludes, such as build output and dependencies, in the explorer.")
                    .switch_only()
                    .child(
                        toggle("Show excluded files", settings.show_excluded_files).on_change(
                            cx.listener(|this, next: &bool, _, cx| {
                                let next = *next;
                                this.update(cx, |state, cx| state.on_show_excluded_files(next, cx))
                            }),
                        ),
                    ),
            );

        div()
            .flex()
            .flex_col()
            .child(theme_group)
            .child(color_group)
            .child(translucency)
            .child(chat_background)
            .child(layout)
    }
}
