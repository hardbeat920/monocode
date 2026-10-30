//! App-wide preferences the Settings pages edit. Defaults are the React
//! app's (`settings.ts`, `appearance.ts`, `displayPrefs.ts`, `sounds.ts`).
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Preferences {
    // General
    pub sounds: bool,
    pub notifications: bool,
    pub project_notifications: serde_json::Value,
    pub notification_projects: serde_json::Value,
    pub file_tabs: String,
    pub file_word_wrap: bool,
    pub file_view_mode: String,
    pub tab_animations: bool,
    pub notes: bool,
    pub quick_composer: bool,
    pub working_agents: bool,
    pub close_to_tray: bool,
    /// Running an action shows the terminal it runs in.
    pub action_opens_terminal: bool,
    /// The new layout opens the terminal as a right panel tab, not a dock.
    pub terminal_in_panel: bool,
    // Appearance
    pub theme: String,
    pub wayland_window_decorations: bool,
    pub workspace_theme: String,
    // Retain the legacy inset-only key when reading and saving preferences.
    // Window materials now use the shared opacity and main-pane glass values.
    pub inset_glass: bool,
    pub accent_color: Option<String>,
    pub diff_palette: String,
    pub theme_hue: u32,
    pub theme_saturation: u32,
    pub theme_dark_lightness: u32,
    pub sidebar_opacity: u32,
    // Retain the saved value for compatibility; native window blur is fixed.
    pub sidebar_blur: u32,
    pub body_glass: bool,
    pub chat_background: Option<String>,
    pub chat_background_effect: String,
    pub chat_background_scope: String,
    pub chat_background_empty: u32,
    pub chat_background_session: u32,
    pub collapsed_project_rail: String,
    pub ui_scale: u32,
    pub show_excluded_files: bool,
    // Chat
    pub transcript_layout: String,
    pub anchor_prompts: bool,
    pub paced_reveal: bool,
    pub follow_up: String,
    pub model_controls: String,
    pub autocorrect: bool,
    pub format_on_save: bool,
    pub autosave: bool,
    pub diff_view: String,
    pub composer_mascot: bool,
    pub empty_session_games: bool,
    // Providers
    pub show_remaining_usage: bool,
    pub mask_emails: bool,
    pub idle_agents: u32,
    pub claude_hooks: bool,
    /// Experimental: report Monocode instead of `sdk-cli` as Claude's entrypoint.
    pub claude_entrypoint: bool,
    pub binary_paths: HashMap<String, String>,
    pub disabled_skill_paths: Vec<String>,
    pub skill_source_paths: Vec<String>,
    pub default_provider: Option<String>,
    pub default_models: HashMap<String, String>,
    pub hidden_providers: Vec<String>,
    pub project_providers: HashMap<String, ProviderDefaults>,
    pub favorite_models: Vec<ModelChoice>,
    pub recent_models: Vec<ModelChoice>,
    pub model_picker_tab: Option<String>,
    /// The last access mode picked in a composer; new chats start with it.
    pub default_access: Option<String>,
    /// Last explicit effort choice for each provider, shared across projects.
    pub provider_efforts: HashMap<String, String>,
    // Monos
    pub monos: bool,
    pub rail_monos_pinned: bool,
    pub mono_menu_bar_icon: bool,
    // Archive
    pub show_archived: bool,
    // Keybindings: a changed shortcut, or `None` for a disabled one.
    pub keybindings: HashMap<String, Option<String>>,
}

pub(crate) const UI_SCALE_PERCENTS: [u32; 16] = [
    50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160, 170, 180, 190, 200,
];

impl Default for Preferences {
    fn default() -> Self {
        Self {
            sounds: true,
            notifications: false,
            project_notifications: serde_json::json!({}),
            notification_projects: serde_json::json!([]),
            file_tabs: "pane".into(),
            file_word_wrap: true,
            file_view_mode: "preview".into(),
            tab_animations: false,
            notes: true,
            quick_composer: true,
            working_agents: true,
            close_to_tray: true,
            action_opens_terminal: true,
            terminal_in_panel: false,
            theme: "dark".into(),
            wayland_window_decorations: true,
            workspace_theme: "default".into(),
            inset_glass: true,
            accent_color: None,
            diff_palette: "default".into(),
            theme_hue: 240,
            theme_saturation: 0,
            theme_dark_lightness: 9,
            sidebar_opacity: 85,
            sidebar_blur: 24,
            body_glass: true,
            chat_background: None,
            chat_background_effect: "none".into(),
            chat_background_scope: "all".into(),
            chat_background_empty: 24,
            chat_background_session: 24,
            collapsed_project_rail: "compact".into(),
            ui_scale: 100,
            show_excluded_files: false,
            transcript_layout: "chat".into(),
            anchor_prompts: true,
            paced_reveal: true,
            follow_up: "steer".into(),
            model_controls: "menu".into(),
            autocorrect: false,
            format_on_save: true,
            autosave: false,
            diff_view: "editor".into(),
            composer_mascot: true,
            empty_session_games: true,
            show_remaining_usage: false,
            mask_emails: true,
            idle_agents: 3,
            claude_hooks: true,
            claude_entrypoint: false,
            binary_paths: HashMap::new(),
            disabled_skill_paths: Vec::new(),
            skill_source_paths: Vec::new(),
            default_provider: None,
            default_models: HashMap::new(),
            hidden_providers: Vec::new(),
            project_providers: HashMap::new(),
            favorite_models: Vec::new(),
            recent_models: Vec::new(),
            model_picker_tab: None,
            default_access: None,
            provider_efforts: HashMap::new(),
            monos: true,
            rail_monos_pinned: false,
            mono_menu_bar_icon: true,
            show_archived: false,
            keybindings: HashMap::new(),
        }
    }
}

impl Preferences {
    pub(crate) fn apply_workspace_theme(&mut self, id: String) {
        let color: gpui::Hsla = gpui::rgb(super::palette(&id, false).surface).into();
        self.theme_hue = (color.h * 360.).round() as u32;
        self.theme_saturation = (color.s * 100.).round() as u32;
        self.theme_dark_lightness = ((color.l * 100.).round() as u32).min(30);
        self.workspace_theme = id;
    }

    /// The color settings a project copies when it gets its own theme.
    pub(crate) fn project_theme(&self) -> monocode_core::project_appearance::ProjectTheme {
        monocode_core::project_appearance::ProjectTheme {
            workspace_theme: self.workspace_theme.clone(),
            accent_color: self.accent_color.clone(),
            hue: self.theme_hue,
            saturation: self.theme_saturation,
            dark_lightness: self.theme_dark_lightness,
        }
    }

    pub(crate) fn wayland_decorations(&self, compositor: &str) -> Option<gpui::WindowDecorations> {
        (compositor == "Wayland").then_some(if self.wayland_window_decorations {
            gpui::WindowDecorations::Server
        } else {
            gpui::WindowDecorations::Client
        })
    }

    /// SettingsView's Restore defaults: the Appearance page only.
    pub(crate) fn restore_appearance(&mut self) {
        let d = Self::default();
        self.theme = d.theme;
        self.wayland_window_decorations = d.wayland_window_decorations;
        self.accent_color = d.accent_color;
        self.workspace_theme = d.workspace_theme;
        self.sidebar_opacity = d.sidebar_opacity;
        self.sidebar_blur = d.sidebar_blur;
        self.theme_hue = d.theme_hue;
        self.theme_saturation = d.theme_saturation;
        self.theme_dark_lightness = d.theme_dark_lightness;
        self.body_glass = d.body_glass;
        self.inset_glass = d.inset_glass;
        self.show_excluded_files = d.show_excluded_files;
        self.chat_background_empty = d.chat_background_empty;
        self.chat_background_session = d.chat_background_session;
        self.chat_background_scope = d.chat_background_scope;
        self.diff_palette = d.diff_palette;
        self.chat_background_effect = d.chat_background_effect;
        self.chat_background = d.chat_background;
        self.ui_scale = d.ui_scale;
        self.collapsed_project_rail = d.collapsed_project_rail;
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct ProviderDefaults {
    pub provider: Option<String>,
    pub models: HashMap<String, String>,
    pub hidden: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ModelChoice {
    pub provider: String,
    pub model: Option<String>,
}

impl gpui::Global for Preferences {}
impl Preferences {
    pub(crate) fn hidden_for(&self, project: &str) -> Vec<String> {
        let mut hidden = self.hidden_providers.clone();
        if let Some(settings) = self.project_providers.get(project) {
            hidden.extend(settings.hidden.clone());
        }
        hidden.sort();
        hidden.dedup();
        hidden
    }
}

#[cfg(test)]
mod decoration_tests {
    use super::Preferences;

    #[test]
    fn decoration_requests_are_scoped_to_the_wayland_backend() {
        for enabled in [true, false] {
            let prefs = Preferences {
                wayland_window_decorations: enabled,
                ..Default::default()
            };
            for compositor in ["X11", "Headless", ""] {
                assert_eq!(prefs.wayland_decorations(compositor), None);
            }
            assert_eq!(
                prefs.wayland_decorations("Wayland"),
                Some(if enabled {
                    gpui::WindowDecorations::Server
                } else {
                    gpui::WindowDecorations::Client
                })
            );
        }
    }

    #[test]
    fn decorations_default_persist_and_restore() {
        let mut prefs: Preferences = serde_json::from_str("{}").unwrap();
        assert!(prefs.wayland_window_decorations);
        prefs.wayland_window_decorations = false;
        let saved = serde_json::to_string(&prefs).unwrap();
        let mut restored: Preferences = serde_json::from_str(&saved).unwrap();
        assert!(!restored.wayland_window_decorations);
        restored.restore_appearance();
        assert!(restored.wayland_window_decorations);
    }
}
