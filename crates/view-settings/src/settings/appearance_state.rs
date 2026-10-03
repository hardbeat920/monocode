//! Port of `useAppearanceSettings` in SettingsView.tsx: the appearance
//! values the page edits, each change saved to `Kv` and applied at once.
//!
//! The webview applied a change by setting a CSS variable or a root class.
//! Here every change recomputes `monocode_ui`'s theme through
//! `set_appearance`, which also re-applies window glass and the interface
//! scale. The chat background goes to [`ChatBackground`].

use std::rc::Rc;

use gpui::{App, Context, Task, Window};
use monocode_core::Platform;
use monocode_core::appearance::*;
use monocode_core::js;
use monocode_core::settings::{COLLAPSED_PROJECT_RAIL_MODE_DEFAULT, CollapsedProjectRailMode};
use monocode_settings::settings_store;
use monocode_settings::{Kv, Subscription};
use monocode_ui::Theme;

use super::background::ChatBackground;
use super::controls::watch_keys;
use super::host::AppearanceHost;
use super::store;

type RailModeHandler = Rc<dyn Fn(CollapsedProjectRailMode, &mut Window, &mut App)>;

pub struct AppearanceState {
    kv: Kv,
    platform: Platform,
    host: Rc<dyn AppearanceHost>,
    /// The values as stored.
    pub settings: AppearanceSettings,
    stored_rail_mode: CollapsedProjectRailMode,
    controlled_rail_mode: Option<CollapsedProjectRailMode>,
    on_rail_mode_change: Option<RailModeHandler>,
    pub chat_background_busy: bool,
    pub chat_background_error: Option<String>,
    background_job: Option<Task<()>>,
    _watch: (Vec<Subscription>, Task<()>),
}

impl AppearanceState {
    pub fn new(
        kv: Kv,
        platform: Platform,
        host: Rc<dyn AppearanceHost>,
        cx: &mut Context<Self>,
    ) -> Self {
        let settings = store::load_appearance(&kv, platform);
        let stored_rail_mode = settings_store::load_collapsed_project_rail_mode(&kv);
        // `subscribeUiScale`: the zoom keys change the scale behind the page.
        let watch = watch_keys(
            &kv,
            &[UI_SCALE_KEY],
            |this: &mut Self, cx| {
                this.settings.ui_scale = store::load_ui_scale(&this.kv);
                cx.notify();
            },
            cx,
        );
        Self {
            kv,
            platform,
            host,
            settings,
            stored_rail_mode,
            controlled_rail_mode: None,
            on_rail_mode_change: None,
            chat_background_busy: false,
            chat_background_error: None,
            background_job: None,
            _watch: watch,
        }
    }

    /// The app shell's rail mode prop and its change callback.
    pub fn set_controlled_rail_mode(
        &mut self,
        mode: Option<CollapsedProjectRailMode>,
        on_change: Option<RailModeHandler>,
    ) {
        self.controlled_rail_mode = mode;
        self.on_rail_mode_change = on_change;
    }

    pub fn collapsed_project_rail_mode(&self) -> CollapsedProjectRailMode {
        self.controlled_rail_mode.unwrap_or(self.stored_rail_mode)
    }

    pub fn kv(&self) -> &Kv {
        &self.kv
    }

    pub fn platform(&self) -> Platform {
        self.platform
    }

    /// Recomputes the theme from the current values.
    fn apply_theme(&self, cx: &mut App) {
        monocode_ui::set_appearance(store::ui_appearance(&self.settings), cx);
    }

    fn is_light(cx: &App) -> bool {
        Theme::of(cx).scheme == monocode_ui::ColorScheme::Light
    }

    pub fn on_theme_preference(&mut self, next: ThemePreference, cx: &mut Context<Self>) {
        store::save_theme_preference(&self.kv, next);
        self.settings.theme_preference = next;
        self.apply_theme(cx);
        // `applyThemePreference` redraws a scheme-dependent background.
        if self.settings.chat_background_path.is_some() {
            ChatBackground::rerender(
                self.settings.new_thread_background_effect,
                Self::is_light(cx),
                cx,
            );
        }
        cx.notify();
    }

    pub fn on_accent_color(&mut self, value: Option<String>, cx: &mut Context<Self>) {
        let next = normalize_accent_color(value.as_deref());
        store::save_accent_color(&self.kv, next.as_deref());
        self.settings.accent_color = next;
        self.apply_theme(cx);
        cx.notify();
    }

    pub fn on_opacity(&mut self, percent: f64, cx: &mut Context<Self>) {
        let next = clamp_sidebar_opacity(percent / 100.0);
        store::save_sidebar_opacity(&self.kv, next);
        self.settings.sidebar_opacity = next;
        self.apply_theme(cx);
        cx.notify();
    }

    pub fn on_main_opacity(&mut self, percent: f64, cx: &mut Context<Self>) {
        let next = clamp_main_opacity(percent / 100.0);
        store::save_main_opacity(&self.kv, next);
        self.settings.main_opacity = next;
        self.apply_theme(cx);
        cx.notify();
    }

    pub fn on_blur(&mut self, radius: f64, cx: &mut Context<Self>) {
        let next = clamp_sidebar_blur(radius);
        self.host.set_window_background_blur(next, cx);
        store::save_sidebar_blur(&self.kv, next as f64);
        self.settings.sidebar_blur = next;
        self.apply_theme(cx);
        cx.notify();
    }

    pub fn on_tint(&mut self, hue: f64, saturation: f64, cx: &mut Context<Self>) {
        let (hue, saturation) = (clamp_theme_hue(hue), clamp_theme_saturation(saturation));
        store::save_theme_hue(&self.kv, hue as f64);
        store::save_theme_saturation(&self.kv, saturation as f64);
        self.settings.theme_hue = hue;
        self.settings.theme_saturation = saturation;
        self.apply_theme(cx);
        cx.notify();
    }

    pub fn on_dark_lightness(&mut self, value: f64, cx: &mut Context<Self>) {
        let next = clamp_theme_dark_lightness(value);
        store::save_theme_dark_lightness(&self.kv, next as f64);
        self.settings.theme_dark_lightness = next;
        self.apply_theme(cx);
        cx.notify();
    }

    pub fn on_body_glass(&mut self, next: bool, cx: &mut Context<Self>) {
        store::save_body_glass(&self.kv, next);
        self.settings.body_glass = next;
        // The theme recomputes native glass, which covers Linux's
        // `syncNativeGlass` call.
        self.apply_theme(cx);
        cx.notify();
    }

    pub fn on_show_excluded_files(&mut self, next: bool, cx: &mut Context<Self>) {
        store::save_show_excluded_files(&self.kv, next);
        self.settings.show_excluded_files = next;
        cx.notify();
    }

    /// `onChooseChatBackground`.
    pub fn on_choose_chat_background(&mut self, cx: &mut Context<Self>) {
        self.chat_background_busy = true;
        self.chat_background_error = None;
        let picked = self.host.pick_and_save_chat_background(cx);
        self.background_job = Some(cx.spawn(async move |this, cx| {
            let result = picked.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(Some(path)) => {
                        store::save_chat_background_path(&this.kv, Some(&path));
                        ChatBackground::apply_chat_background(
                            Some(&path),
                            this.settings.new_thread_background_effect,
                            Self::is_light(cx),
                            cx,
                        );
                        this.settings.chat_background_path = Some(path);
                    }
                    Ok(None) => {}
                    Err(error) => this.chat_background_error = Some(error),
                }
                this.chat_background_busy = false;
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    /// `onClearChatBackground`.
    pub fn on_clear_chat_background(&mut self, cx: &mut Context<Self>) {
        self.chat_background_busy = true;
        self.chat_background_error = None;
        let removed = self.host.remove_chat_background(cx);
        self.background_job = Some(cx.spawn(async move |this, cx| {
            let result = removed.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(()) => {
                        store::save_chat_background_path(&this.kv, None);
                        ChatBackground::apply_chat_background(
                            None,
                            this.settings.new_thread_background_effect,
                            false,
                            cx,
                        );
                        this.settings.chat_background_path = None;
                    }
                    Err(error) => this.chat_background_error = Some(error),
                }
                this.chat_background_busy = false;
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    pub fn on_chat_background_empty_opacity(&mut self, percent: f64, cx: &mut Context<Self>) {
        let next = clamp_chat_background_opacity(percent / 100.0);
        store::save_chat_background_empty_opacity(&self.kv, next);
        self.settings.chat_background_empty_opacity = next;
        self.apply_theme(cx);
        cx.notify();
    }

    pub fn on_chat_background_session_opacity(&mut self, percent: f64, cx: &mut Context<Self>) {
        let next = clamp_chat_background_opacity(percent / 100.0);
        store::save_chat_background_session_opacity(&self.kv, next);
        self.settings.chat_background_session_opacity = next;
        self.apply_theme(cx);
        cx.notify();
    }

    pub fn on_chat_background_blur(&mut self, radius: f64, cx: &mut Context<Self>) {
        let next = clamp_chat_background_blur(radius);
        store::save_chat_background_blur(&self.kv, next as f64);
        self.settings.chat_background_blur = next;
        self.apply_theme(cx);
        cx.notify();
    }

    pub fn on_chat_background_scope(&mut self, next: ChatBackgroundScope, cx: &mut Context<Self>) {
        store::save_chat_background_scope(&self.kv, next);
        self.settings.chat_background_scope = next;
        cx.notify();
    }

    /// `setNewThreadBackgroundEffect`: save, then redraw the background.
    pub fn on_new_thread_background_effect(
        &mut self,
        next: NewThreadBackgroundEffect,
        cx: &mut Context<Self>,
    ) {
        store::save_new_thread_background_effect(&self.kv, next);
        self.settings.new_thread_background_effect = next;
        if self.settings.chat_background_path.is_some() {
            ChatBackground::rerender(next, Self::is_light(cx), cx);
        }
        cx.notify();
    }

    pub fn on_ui_scale(&mut self, percent: f64, cx: &mut Context<Self>) {
        let next = store::save_ui_scale(&self.kv, percent / 100.0);
        self.settings.ui_scale = next;
        self.apply_theme(cx);
        cx.notify();
    }

    pub fn on_collapsed_project_rail_mode(
        &mut self,
        next: CollapsedProjectRailMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        settings_store::save_collapsed_project_rail_mode(&self.kv, next);
        self.stored_rail_mode = next;
        if let Some(on_change) = self.on_rail_mode_change.clone() {
            window.defer(cx, move |window, cx| on_change(next, window, cx));
        }
        cx.notify();
    }

    /// `restoreDefaults`: every appearance value back to its default, the
    /// chat background removed, and the rail back to the icon rail.
    pub fn restore_defaults(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let defaults = AppearanceSettings::defaults_for(self.platform);
        self.on_theme_preference(THEME_PREFERENCE_DEFAULT, cx);
        self.on_accent_color(ACCENT_COLOR_DEFAULT.map(str::to_string), cx);
        self.on_opacity(js::round(SIDEBAR_OPACITY_DEFAULT * 100.0), cx);
        self.on_main_opacity(js::round(MAIN_OPACITY_DEFAULT * 100.0), cx);
        self.on_blur(SIDEBAR_BLUR_DEFAULT, cx);
        self.on_tint(THEME_HUE_DEFAULT, THEME_SATURATION_DEFAULT, cx);
        self.on_dark_lightness(THEME_DARK_LIGHTNESS_DEFAULT, cx);
        self.on_body_glass(defaults.body_glass, cx);
        self.on_show_excluded_files(SHOW_EXCLUDED_FILES_DEFAULT, cx);
        self.on_chat_background_empty_opacity(
            js::round(CHAT_BACKGROUND_EMPTY_OPACITY_DEFAULT * 100.0),
            cx,
        );
        self.on_chat_background_session_opacity(
            js::round(CHAT_BACKGROUND_SESSION_OPACITY_DEFAULT * 100.0),
            cx,
        );
        self.on_chat_background_scope(CHAT_BACKGROUND_SCOPE_DEFAULT, cx);
        self.on_chat_background_blur(CHAT_BACKGROUND_BLUR_DEFAULT, cx);
        self.on_new_thread_background_effect(NEW_THREAD_BACKGROUND_EFFECT_DEFAULT, cx);
        if self.settings.chat_background_path.is_some() {
            self.on_clear_chat_background(cx);
        }
        self.on_ui_scale(js::round(UI_SCALE_DEFAULT * 100.0), cx);
        self.on_collapsed_project_rail_mode(COLLAPSED_PROJECT_RAIL_MODE_DEFAULT, window, cx);
    }
}
