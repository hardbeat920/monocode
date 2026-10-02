//! Port of src/features/settings/model/settings.ts and storageFlags.ts:
//! settings sections and search, app behavior settings with their defaults,
//! and the keybinding table with user overrides.
//!
//! Persistence stays out of this crate. Each setting names the localStorage
//! key the TypeScript stored it under (the `*_KEY` constants), and each
//! `parse_*` function reads that stored string the way the matching `load*`
//! function did. `Settings::from_local_storage` reads them all.

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use crate::js;
use crate::platform::Platform;
use crate::shortcut::{
    QUICK_COMPOSER_DEFAULT_SHORTCUT, ShortcutEvent, canonical_shortcut, is_function_key,
    is_global_shortcut, quick_composer_shortcut_label, shortcut_from_key_event, shortcut_tokens,
};

/// `readFlag`: `"1"` and `"true"` are on, any other stored string is off,
/// and a missing value is `None`.
pub fn read_flag(raw: Option<&str>) -> Option<bool> {
    raw.map(|raw| raw == "1" || raw == "true")
}

/// `writeFlag`: the string a flag is stored as.
pub fn write_flag(value: bool) -> &'static str {
    if value { "1" } else { "0" }
}

/// `readNumber` in appearance.ts: `Number(raw)` when finite.
pub fn read_number(raw: Option<&str>) -> Option<f64> {
    js::parse_number(raw?).filter(|n| n.is_finite())
}

macro_rules! string_enum {
    (
        $(#[$meta:meta])*
        $name:ident { $($(#[$vmeta:meta])* $variant:ident = $value:literal),+ $(,)? }
        default $default:ident
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
        pub enum $name {
            $(
                $(#[$vmeta])*
                #[serde(rename = $value)]
                $variant,
            )+
        }

        impl $name {
            pub const ALL: &'static [$name] = &[$($name::$variant),+];
            pub const DEFAULT: $name = $name::$default;

            pub const fn as_str(self) -> &'static str {
                match self {
                    $($name::$variant => $value,)+
                }
            }

            /// The value for a stored string, or `None` when it is unknown.
            pub fn from_str_opt(value: &str) -> Option<Self> {
                match value {
                    $($value => Some($name::$variant),)+
                    _ => None,
                }
            }

            /// The `load*` behavior: the stored value when known, else the default.
            pub fn parse(raw: Option<&str>) -> Self {
                raw.and_then(Self::from_str_opt).unwrap_or(Self::DEFAULT)
            }
        }
    };
}
pub(crate) use string_enum;

string_enum! {
    /// `SettingsSectionId`.
    SettingsSectionId {
        #[default]
        General = "general",
        Connections = "connections",
        Appearance = "appearance",
        Keybindings = "keybindings",
        Chat = "chat",
        Providers = "providers",
        Mcp = "mcp",
        Skills = "skills",
        Inbox = "inbox",
        Worktrees = "worktrees",
        Archive = "archive",
    }
    default General
}

string_enum! {
    /// `SettingsGroupId`: rail buckets. Sections list in order under their group label.
    SettingsGroupId {
        #[default]
        App = "app",
        Agents = "agents",
        Workspace = "workspace",
    }
    default App
}

string_enum! {
    /// `FollowUpBehavior`: what sending while a turn runs does.
    FollowUpBehavior {
        #[default]
        Steer = "steer",
        Queue = "queue",
    }
    default Steer
}

string_enum! {
    /// `FileTabMode`: whether an ordinary file joins the active pane or gets a top tab.
    FileTabMode {
        #[default]
        Pane = "pane",
        Workspace = "workspace",
    }
    default Pane
}

string_enum! {
    /// `CollapsedProjectRailMode`.
    CollapsedProjectRailMode {
        #[default]
        Compact = "compact",
        Hidden = "hidden",
    }
    default Compact
}

string_enum! {
    /// `ModelControls`: where the composer shows effort and other model options.
    ModelControls {
        #[default]
        Menu = "menu",
        Beside = "beside",
    }
    default Menu
}

string_enum! {
    /// `DiffViewer`: the working-tree diff layout.
    DiffViewer {
        #[default]
        Editor = "editor",
        Unified = "unified",
    }
    default Editor
}

/// `SETTINGS_GROUPS`.
pub const SETTINGS_GROUPS: [(SettingsGroupId, &str); 3] = [
    (SettingsGroupId::App, "App"),
    (SettingsGroupId::Agents, "Agents"),
    (SettingsGroupId::Workspace, "Workspace"),
];

/// `SettingsSection`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettingsSection {
    pub id: SettingsSectionId,
    pub group: SettingsGroupId,
    pub label: &'static str,
    pub description: &'static str,
    /// Extra words search matches the section on, beyond its label.
    pub keywords: Option<&'static str>,
}

const fn section(
    id: SettingsSectionId,
    group: SettingsGroupId,
    label: &'static str,
    description: &'static str,
    keywords: &'static str,
) -> SettingsSection {
    SettingsSection {
        id,
        group,
        label,
        description,
        keywords: Some(keywords),
    }
}

/// `SETTINGS_SECTIONS`.
pub const SETTINGS_SECTIONS: [SettingsSection; 11] = {
    use SettingsGroupId as G;
    use SettingsSectionId as S;
    [
        section(
            S::General,
            G::App,
            "General",
            "The build you are running, how MonoCode reaches you, and the panels it shows.",
            "version update sounds notifications notes rail",
        ),
        section(
            S::Connections,
            G::App,
            "Connections",
            "Connect your machines and run agents remotely through SSH.",
            "ssh remote host machine server environment always on",
        ),
        section(
            S::Appearance,
            G::App,
            "Appearance",
            "Theme, tint, translucency, workspace layout, and conversation backgrounds.",
            "theme dark light color accent glass blur zoom scale wallpaper rail sidebar",
        ),
        section(
            S::Keybindings,
            G::App,
            "Keybindings",
            "Every shortcut the workspace handles, from the app menu and the key handler.",
            "shortcut hotkey keyboard binding",
        ),
        section(
            S::Chat,
            G::Agents,
            "Chat",
            "How transcripts read, what the composer does with a follow-up, how files save, and how diffs open.",
            "transcript composer prompt message diff review layout format save editor",
        ),
        section(
            S::Providers,
            G::Agents,
            "Providers",
            "Provider accounts, agent CLIs MonoCode can drive, and the model new sessions start with.",
            "account sign in login model harness claude codex gemini cli default hooks",
        ),
        section(
            S::Mcp,
            G::Agents,
            "MCP",
            "Find MCP servers across providers and manage their connections.",
            "tools servers connections oauth authenticate login claude codex cursor opencode",
        ),
        section(
            S::Skills,
            G::Agents,
            "Skills",
            "Discover and manage file skills from project, personal, and harness folders.",
            "skill instructions prompt",
        ),
        section(
            S::Inbox,
            G::Workspace,
            "Inbox",
            "Manage Inbox services and notification preferences for each project.",
            "github gitlab linear jira atlassian azure devops connect token integration",
        ),
        section(
            S::Archive,
            G::Workspace,
            "Archive",
            "Projects and conversations you have archived.",
            "archived restore delete hidden",
        ),
        section(
            S::Worktrees,
            G::Workspace,
            "Worktrees",
            "Manage additional worktrees for each project.",
            "git branch worktree working copy project create delete",
        ),
    ]
};

/// One rail group with its sections.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsGroup {
    pub id: SettingsGroupId,
    pub label: &'static str,
    pub sections: Vec<SettingsSection>,
}

/// `settingsSectionsByGroup`.
pub fn settings_sections_by_group() -> Vec<SettingsGroup> {
    SETTINGS_GROUPS
        .iter()
        .map(|(id, label)| SettingsGroup {
            id: *id,
            label,
            sections: SETTINGS_SECTIONS
                .iter()
                .filter(|section| section.group == *id)
                .copied()
                .collect(),
        })
        .filter(|group| !group.sections.is_empty())
        .collect()
}

/// `SettingsEntry`: one searchable control. `id` is the row Settings scrolls to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettingsEntry {
    pub id: &'static str,
    pub section: SettingsSectionId,
    pub label: &'static str,
    pub keywords: Option<&'static str>,
}

/// `SETTINGS_INDEX`. Quick Composer appears only on macOS and Close to tray
/// only on Windows.
pub fn settings_index(platform: Platform) -> Vec<SettingsEntry> {
    use SettingsSectionId as S;
    let entry = |id, section, label, keywords| SettingsEntry {
        id,
        section,
        label,
        keywords: Some(keywords),
    };
    let mut index = vec![
        entry(
            "remote-machines",
            S::Connections,
            "Your machines",
            "ssh remote connect host server environment",
        ),
        entry(
            "mcp-servers",
            S::Mcp,
            "MCP servers",
            "claude tools connections oauth authenticate login add remove",
        ),
        entry(
            "project-worktrees",
            S::Worktrees,
            "Project worktrees",
            "git branch working copy create delete manage",
        ),
        entry(
            "update",
            S::General,
            "Version",
            "update upgrade release what's new build changelog",
        ),
        entry(
            "sounds",
            S::General,
            "Sounds",
            "audio cue chime mute volume",
        ),
        entry(
            "notifications",
            S::General,
            "Notifications",
            "notify alert toast permission reminder background",
        ),
        entry(
            "notes",
            S::General,
            "Notes",
            "notebook markdown rail scratchpad",
        ),
    ];
    if platform.is_mac() {
        index.push(entry(
            "quick-composer",
            S::General,
            "Quick composer",
            "spotlight global shortcut hotkey floating prompt anywhere",
        ));
    }
    index.extend([
        entry(
            "working-agents",
            S::General,
            "Working agents",
            "live running sessions rail card",
        ),
        entry(
            "file-tabs",
            S::General,
            "File tabs",
            "editor open top workspace normal session pane beside chat",
        ),
        entry(
            "tab-animations",
            S::General,
            "Tab animations",
            "motion open close resize transition",
        ),
    ]);
    if platform.is_windows() {
        index.push(entry(
            "close-to-tray",
            S::General,
            "Close to tray",
            "minimize background quit exit window taskbar windows",
        ));
    }
    index.extend([
        entry("theme", S::Appearance, "Theme", "dark light system appearance mode"),
        entry("accent-color", S::Appearance, "Accent color", "highlight bubble send button tint"),
        entry("hue", S::Appearance, "Hue", "tint color chrome"),
        entry("saturation", S::Appearance, "Saturation", "tint color neutral grey gray"),
        entry("dark-lightness", S::Appearance, "Dark-mode lightness", "black brightness contrast background"),
        entry("sidebar-opacity", S::Appearance, "Sidebar opacity", "glass translucent transparency vibrancy rail"),
        entry("blur", S::Appearance, "Blur radius", "glass translucent vibrancy backdrop"),
        entry("main-pane-glass", S::Appearance, "Main pane glass", "translucent transparency body window"),
        entry("main-pane-opacity", S::Appearance, "Main pane opacity", "glass translucent transparency body window"),
        entry("interface-scale", S::Appearance, "Interface scale", "zoom font size bigger smaller ui"),
        entry("collapsed-project-rail", S::Appearance, "Collapsed project rail", "sidebar compact icons hidden navigation layout"),
        entry("show-excluded-files", S::Appearance, "Show excluded files", "explorer gitignore ignored hidden files tree"),
        entry("chat-background", S::Appearance, "Chat background", "wallpaper image picture opacity backdrop blur"),
        entry("transcript-layout", S::Chat, "Transcript layout", "full width chat bubble message"),
        entry("anchor-prompts", S::Chat, "Anchor prompts to top", "scroll position sticky message"),
        entry("follow-up", S::Chat, "Follow-up behavior", "queue steer interrupt send while running"),
        entry("model-controls", S::Chat, "Model controls", "effort thinking reasoning fast service tier model picker composer"),
        entry("composer-mascot", S::Chat, "Composer mascot", "runner animation coin fun"),
        entry("format-on-save", S::Chat, "Format on save", "prettier quotes editor save format"),
        entry("diff-view", S::Chat, "Diff view", "unified editor review changes working tree"),
        entry("empty-session-games", S::Chat, "Empty session games", "pacman snake arcade grid fun"),
        entry("agent-clis", S::Providers, "Agent CLIs", "codex opencode cursor grok pi omp fx hermes antigravity binary path"),
        entry("provider-accounts", S::Providers, "Provider accounts", "account sign in login rename remove delete credentials profile usage limit quota exhausted"),
        entry("claude-hooks", S::Providers, "Claude Code hooks", "pretooluse settings.json block command notification"),
        entry("project-notifications", S::Inbox, "Project notifications", "mute resume sounds banners reminders categories"),
        entry("github", S::Inbox, "GitHub", "gh cli connect pull request sign in"),
        entry("gitlab", S::Inbox, "GitLab", "token self-managed merge request connect"),
        entry("azuredevops", S::Inbox, "ADO", "azure devops boards repos pull request pat organization connect"),
        entry("jira", S::Inbox, "Jira", "atlassian cloud site email api token issues projects connect"),
        entry("linear", S::Inbox, "Linear", "api key issues teams connect"),
        entry("show-archived", S::Archive, "Show archived in the sidebar", "hidden conversations list"),
    ]);
    index
}

/// `SettingsSearchResult`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsSearchResult {
    pub section: SettingsSectionId,
    pub section_label: &'static str,
    /// Row to scroll to, or `None` when the whole section matched.
    pub setting_id: Option<&'static str>,
    pub label: &'static str,
}

/// `matchScore`, doubled so a section's half-point penalty stays an integer.
fn match_score(needle: &str, label: &str, keywords: Option<&str>) -> Option<u32> {
    let lower = label.to_lowercase();
    if lower.starts_with(needle) {
        return Some(0);
    }
    if lower.contains(needle) {
        return Some(2);
    }
    if keywords.is_some_and(|keywords| keywords.to_lowercase().contains(needle)) {
        return Some(4);
    }
    None
}

/// Approximation of `String.prototype.localeCompare` for settings labels:
/// case-insensitive first, then lowercase before uppercase.
// TODO(port): localeCompare uses ICU collation. This matches it for the
// ASCII labels in the settings index.
pub(crate) fn locale_compare(a: &str, b: &str) -> Ordering {
    a.to_lowercase()
        .cmp(&b.to_lowercase())
        .then_with(|| b.cmp(a))
}

/// `searchSettings`: individual settings first, then whole sections, so a row
/// wins its own name. The TypeScript default `limit` is 8.
pub fn search_settings(query: &str, limit: usize, platform: Platform) -> Vec<SettingsSearchResult> {
    let needle = js::trim(query).to_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }
    let mut scored: Vec<(u32, SettingsSearchResult)> = Vec::new();
    for entry in settings_index(platform) {
        if let Some(score) = match_score(&needle, entry.label, entry.keywords) {
            scored.push((
                score,
                SettingsSearchResult {
                    section: entry.section,
                    section_label: settings_section_label(entry.section),
                    setting_id: Some(entry.id),
                    label: entry.label,
                },
            ));
        }
    }
    for section in SETTINGS_SECTIONS {
        let keywords = format!("{} {}", section.description, section.keywords.unwrap_or(""));
        if let Some(score) = match_score(&needle, section.label, Some(&keywords)) {
            scored.push((
                score + 1,
                SettingsSearchResult {
                    section: section.id,
                    section_label: section.label,
                    setting_id: None,
                    label: section.label,
                },
            ));
        }
    }
    scored.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| locale_compare(a.1.label, b.1.label))
    });
    scored
        .into_iter()
        .take(limit)
        .map(|(_, result)| result)
        .collect()
}

/// `SETTINGS_SECTION_DEFAULT`.
pub const SETTINGS_SECTION_DEFAULT: SettingsSectionId = SettingsSectionId::General;

/// `settingsSectionLabel`.
pub fn settings_section_label(id: SettingsSectionId) -> &'static str {
    SETTINGS_SECTIONS
        .iter()
        .find(|section| section.id == id)
        .map(|section| section.label)
        .unwrap_or("General")
}

/// `settingsSectionDescription`.
pub fn settings_section_description(id: SettingsSectionId) -> &'static str {
    SETTINGS_SECTIONS
        .iter()
        .find(|section| section.id == id)
        .map(|section| section.description)
        .unwrap_or("")
}

// localStorage keys, one per setting.
pub const SECTION_KEY: &str = "monocode.settingsSection";
pub const FOLLOW_UP_BEHAVIOR_KEY: &str = "monocode.followUpBehavior";
pub const FILE_TAB_MODE_KEY: &str = "monocode.fileTabMode";
pub const TAB_ANIMATIONS_ENABLED_KEY: &str = "monocode.tabAnimationsEnabled";
pub const COLLAPSED_PROJECT_RAIL_MODE_KEY: &str = "monocode.collapsedProjectRailMode";
pub const MODEL_CONTROLS_KEY: &str = "monocode.modelControls";
/// Legacy toggle `parse_model_controls` migrates: on means beside the picker.
pub const COMPOSER_EFFORT_VISIBLE_KEY: &str = "monocode.composerEffortVisible";
pub const COMPOSER_RUNNER_KEY: &str = "monocode.composerRunner";
pub const NOTES_ENABLED_KEY: &str = "monocode.notesEnabled";
pub const QUICK_COMPOSER_ENABLED_KEY: &str = "monocode.quickComposerEnabled";
pub const QUICK_COMPOSER_SHORTCUT_KEY: &str = "monocode.quickComposerShortcut";
pub const LIVE_AGENTS_ENABLED_KEY: &str = "monocode.liveAgentsEnabled";
pub const CLOSE_TO_TRAY_KEY: &str = "monocode.closeToTray";
pub const GRID_ARCADE_ENABLED_KEY: &str = "monocode.gridArcadeEnabled";
pub const DIFF_VIEWER_KEY: &str = "monocode.diffViewer";
pub const FORMAT_ON_SAVE_KEY: &str = "monocode.formatOnSave";
pub const AUTOSAVE_KEY: &str = "monocode.autosave";
pub const CLAUDE_HOOKS_KEY: &str = "monocode.claudeHooks";
pub const KEYBINDING_OVERRIDES_KEY: &str = "monocode.keybindingOverrides";

// Defaults.
pub const FOLLOW_UP_BEHAVIOR_DEFAULT: FollowUpBehavior = FollowUpBehavior::Steer;
pub const FILE_TAB_MODE_DEFAULT: FileTabMode = FileTabMode::Pane;
pub const TAB_ANIMATIONS_ENABLED_DEFAULT: bool = false;
pub const COLLAPSED_PROJECT_RAIL_MODE_DEFAULT: CollapsedProjectRailMode =
    CollapsedProjectRailMode::Compact;
pub const MODEL_CONTROLS_DEFAULT: ModelControls = ModelControls::Menu;
pub const COMPOSER_RUNNER_DEFAULT: bool = true;
pub const NOTES_ENABLED_DEFAULT: bool = true;
pub const QUICK_COMPOSER_ENABLED_DEFAULT: bool = true;
pub const LIVE_AGENTS_ENABLED_DEFAULT: bool = true;
pub const CLOSE_TO_TRAY_DEFAULT: bool = true;
pub const GRID_ARCADE_ENABLED_DEFAULT: bool = true;
pub const DIFF_VIEWER_DEFAULT: DiffViewer = DiffViewer::Editor;
pub const FORMAT_ON_SAVE_DEFAULT: bool = true;
pub const AUTOSAVE_DEFAULT: bool = false;
pub const CLAUDE_HOOKS_DEFAULT: bool = true;

/// `loadModelControls`: the stored value, else the legacy effort-control
/// toggle when no value was ever stored.
pub fn parse_model_controls(
    raw: Option<&str>,
    legacy_effort_visible: Option<&str>,
) -> ModelControls {
    if let Some(value) = raw.and_then(ModelControls::from_str_opt) {
        return value;
    }
    if raw.is_none() && matches!(legacy_effort_visible, Some("1" | "true")) {
        return ModelControls::Beside;
    }
    MODEL_CONTROLS_DEFAULT
}

/// `loadQuickComposerShortcut`: the stored chord when it is a valid global
/// shortcut, else the default.
pub fn parse_quick_composer_shortcut(raw: Option<&str>) -> String {
    match raw {
        Some(value) if !value.is_empty() && is_global_shortcut(value) => value.to_string(),
        _ => QUICK_COMPOSER_DEFAULT_SHORTCUT.to_string(),
    }
}

/// `KeybindingRow`: one line of the keybindings table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeybindingRow {
    pub command: String,
    pub keys: String,
    pub when: String,
}

/// `QUICK_COMPOSER_COMMAND`.
pub const QUICK_COMPOSER_COMMAND: &str = "App: Quick Composer";
/// `ACTIVATE_RANGE_COMMAND`.
pub const ACTIVATE_RANGE_COMMAND: &str = "Tab: Activate 1–8";

/// `KEYBINDINGS`: the bindings the app handles (menu accelerators, tab
/// commands, the window key handler, and focused surface handlers), with
/// labels for `platform`.
pub fn keybindings(platform: Platform) -> Vec<KeybindingRow> {
    let m = platform.mod_label();
    let s = platform.shift_label();
    let a = platform.alt_label();
    let c = platform.ctrl_label();
    let always = "Always";
    let nav = "!overlay && (!textFocus || emptyComposer)";
    let row = |command: &str, keys: String, when: &str| KeybindingRow {
        command: command.into(),
        keys,
        when: when.into(),
    };
    let mut rows = vec![
        row("App: Settings", format!("{m},"), always),
        row("App: Search", format!("{m}K"), always),
        row("App: Go to File", format!("{m}P"), always),
        row("App: Command Palette", format!("{m}{s}P"), always),
        row("App: Find in Files", format!("{m}{s}F"), always),
        row("App: Open Project", format!("{m}O"), always),
        row("App: New Window", format!("{m}{s}N"), always),
    ];
    if platform.is_mac() {
        rows.push(row(
            QUICK_COMPOSER_COMMAND,
            format!("{m}{s}Space"),
            "Anywhere",
        ));
    }
    rows.extend([
        row("App: Toggle Sidebar", format!("{m}B"), always),
        row("App: Toggle Session Sidebar", format!("{m}{s}B"), always),
        row("App: Switch Model", format!("{m}."), always),
        row(
            "Composer: Toggle Workspace",
            format!("{m}{s}G"),
            "Draft session composer",
        ),
        row("View: Reload", format!("{m}{s}R"), always),
        row("View: Zoom In", format!("{m}+"), always),
        row("View: Zoom Out", format!("{m}-"), always),
        row("View: Reset Zoom", format!("{m}0"), always),
        row("Tab: New", format!("{m}T"), always),
        row("Tab: Close Others", format!("{m}{a}T"), always),
        row("Tab: Close All", format!("{m}{s}W"), always),
        row("Tab: Next", format!("{m}{s}]"), always),
        row("Tab: Previous", format!("{m}{s}["), always),
        row("Tab: Cycle Next", format!("{c}Tab"), always),
        row("Tab: Cycle Previous", format!("{c}{s}Tab"), always),
        row("Tab: Back", format!("{m}["), always),
        row("Tab: Forward", format!("{m}]"), always),
        row(ACTIVATE_RANGE_COMMAND, format!("{m}1 … {m}8"), always),
        row("Tab: Activate Last", format!("{m}9"), always),
        row(
            "Session: Archive",
            format!("{m}{s}A"),
            "sessionFocus && !overlay",
        ),
        row("Session: Previous", format!("{m}{s}↑"), nav),
        row("Session: Next", format!("{m}{s}↓"), nav),
        row("Session: Previous in Current Tab", format!("{m}↑"), nav),
        row("Session: Next in Current Tab", format!("{m}↓"), nav),
        row("Project: Previous", format!("{m}{s}←"), nav),
        row("Project: Next", format!("{m}{s}→"), nav),
        row("Pane: Close", format!("{m}W"), always),
        row("Pane: Split Right", format!("{m}D"), "!editorFocus"),
        row("Pane: Split Down", format!("{m}{s}D"), "!editorFocus"),
        row("Pane: Focus Left", format!("{m}{a}←"), always),
        row("Pane: Focus Right", format!("{m}{a}→"), always),
        row("Pane: Focus Up", format!("{m}{a}↑"), always),
        row("Pane: Focus Down", format!("{m}{a}↓"), always),
        row("Terminal: New", format!("{m}`"), always),
        row("Terminal: New Tab", format!("{m}{s}`"), always),
        row("Terminal: Toggle Dock", format!("{m}J"), always),
        row("Editor: Find", format!("{m}F"), "editorFocus"),
        row("Editor: Replace", format!("{m}{a}F"), "editorFocus"),
    ]);
    rows
}

/// `KeybindingOverride`: a user's rebinding or removal of one command.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeybindingOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shortcut: Option<String>,
}

impl KeybindingOverride {
    pub fn is_disabled(&self) -> bool {
        self.disabled == Some(true)
    }

    fn shortcut(&self) -> Option<&str> {
        self.shortcut
            .as_deref()
            .filter(|shortcut| !shortcut.is_empty())
    }
}

/// `KeybindingOverrides`, keyed by command.
// TODO(port): JavaScript objects keep insertion order; this map sorts by
// command. Only the stored JSON's key order differs.
pub type KeybindingOverrides = BTreeMap<String, KeybindingOverride>;

/// `KEY_CODES`: key labels in the table to `KeyboardEvent.code`.
fn key_code(label: &str) -> Option<&'static str> {
    Some(match label {
        " " | "Space" => "Space",
        "Enter" => "Enter",
        "Tab" => "Tab",
        "`" => "Backquote",
        "[" => "BracketLeft",
        "]" => "BracketRight",
        "," => "Comma",
        "." => "Period",
        "+" => "Equal",
        "-" => "Minus",
        "\\" => "Backslash",
        "↑" => "ArrowUp",
        "↓" => "ArrowDown",
        "←" => "ArrowLeft",
        "→" => "ArrowRight",
        _ => return None,
    })
}

fn display_modifiers(platform: Platform) -> &'static [(&'static str, &'static str)] {
    if platform.is_mac() {
        &[
            ("⌘", "Command"),
            ("⌃", "Control"),
            ("⌥", "Option"),
            ("⇧", "Shift"),
        ]
    } else {
        &[
            ("Ctrl+", "Control"),
            ("Alt+", "Option"),
            ("Shift+", "Shift"),
        ]
    }
}

/// `defaultShortcutsFor`: every chord a command owns by default, in stored
/// form. Grouped rows expand to one chord per key, so a rebind can never
/// shadow a working shortcut.
pub fn default_shortcuts_for(command: &str, platform: Platform) -> Vec<String> {
    let rows = keybindings(platform);
    let Some(row) = rows.iter().find(|row| row.command == command) else {
        return Vec::new();
    };
    let mut rest = row.keys.as_str();
    let mut modifiers: Vec<&str> = Vec::new();
    for (display, modifier) in display_modifiers(platform) {
        if let Some(stripped) = rest.strip_prefix(display) {
            modifiers.push(modifier);
            rest = stripped;
        }
    }
    let chords = |code: &str| -> Vec<String> {
        let joined = if modifiers.is_empty() {
            code.to_string()
        } else {
            format!("{}+{code}", modifiers.join("+"))
        };
        canonical_shortcut(&joined).into_iter().collect()
    };
    if row.keys.contains('…') {
        return (1..=8)
            .flat_map(|digit| chords(&format!("Digit{digit}")))
            .collect();
    }
    let mut letters = rest.chars();
    if let (Some(ch), None) = (letters.next(), letters.next()) {
        if ch.is_ascii_alphabetic() {
            return chords(&format!("Key{}", ch.to_ascii_uppercase()));
        }
        if ch.is_ascii_digit() {
            return chords(&format!("Digit{ch}"));
        }
    }
    if let Some(code) = key_code(rest) {
        return chords(code);
    }
    if is_function_key(rest) {
        return chords(rest);
    }
    Vec::new()
}

/// `parseKeybindingOverrides`: keep known commands with a valid shortcut or
/// a disable flag. Malformed JSON reads as no overrides.
pub fn parse_keybinding_overrides(raw: Option<&str>, platform: Platform) -> KeybindingOverrides {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw.unwrap_or("{}")) else {
        return KeybindingOverrides::new();
    };
    let Some(entries) = value.as_object() else {
        return KeybindingOverrides::new();
    };
    let commands: Vec<String> = keybindings(platform)
        .into_iter()
        .map(|row| row.command)
        .collect();
    let mut next = KeybindingOverrides::new();
    for (command, entry) in entries {
        if !commands.contains(command) {
            continue;
        }
        // typeof entry === "object" also admits arrays, which carry neither field.
        let Some(entry) = entry.as_object() else {
            continue;
        };
        let disabled = entry.get("disabled") == Some(&serde_json::Value::Bool(true));
        let shortcut = entry
            .get("shortcut")
            .and_then(serde_json::Value::as_str)
            .and_then(canonical_shortcut);
        if let Some(shortcut) = shortcut {
            next.insert(
                command.clone(),
                KeybindingOverride {
                    disabled: None,
                    shortcut: Some(shortcut),
                },
            );
        } else if disabled {
            next.insert(
                command.clone(),
                KeybindingOverride {
                    disabled: Some(true),
                    shortcut: None,
                },
            );
        }
    }
    next
}

fn validate_shortcut(command: &str, shortcut: &str) -> Result<String, String> {
    let canonical = canonical_shortcut(shortcut)
        .ok_or_else(|| "That combination is not a valid shortcut".to_string())?;
    if command == ACTIVATE_RANGE_COMMAND {
        let digit_1_to_8 = canonical
            .strip_suffix(|c: char| ('1'..='8').contains(&c))
            .is_some_and(|head| head.ends_with("Digit"));
        if !digit_1_to_8 {
            return Err("Tab: Activate 1–8 needs a number key from 1 to 8".into());
        }
    }
    Ok(canonical)
}

/// `currentKeybindings` row for one command.
fn label_for(override_: Option<&KeybindingOverride>, fallback: &str, platform: Platform) -> String {
    match override_ {
        Some(o) if o.is_disabled() => "Disabled".into(),
        Some(o) => match o.shortcut() {
            Some(shortcut) => quick_composer_shortcut_label(shortcut, platform),
            None => fallback.into(),
        },
        None => fallback.into(),
    }
}

/// App behavior settings from settings.ts. Each field names its storage key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// `SECTION_KEY`: the Settings section last open.
    pub settings_section: SettingsSectionId,
    /// `FOLLOW_UP_BEHAVIOR_KEY`.
    pub follow_up_behavior: FollowUpBehavior,
    /// `FILE_TAB_MODE_KEY`.
    pub file_tab_mode: FileTabMode,
    /// `TAB_ANIMATIONS_ENABLED_KEY`.
    pub tab_animations_enabled: bool,
    /// `COLLAPSED_PROJECT_RAIL_MODE_KEY`.
    pub collapsed_project_rail_mode: CollapsedProjectRailMode,
    /// `MODEL_CONTROLS_KEY`, migrated from `COMPOSER_EFFORT_VISIBLE_KEY`.
    pub model_controls: ModelControls,
    /// `COMPOSER_RUNNER_KEY`: the composer mascot.
    pub composer_runner: bool,
    /// `NOTES_ENABLED_KEY`.
    pub notes_enabled: bool,
    /// `QUICK_COMPOSER_ENABLED_KEY`.
    pub quick_composer_enabled: bool,
    /// `QUICK_COMPOSER_SHORTCUT_KEY`: a global chord, such as `Command+Shift+Space`.
    pub quick_composer_shortcut: String,
    /// `LIVE_AGENTS_ENABLED_KEY`: the working-agents rail card.
    pub live_agents_enabled: bool,
    /// `CLOSE_TO_TRAY_KEY`. The stored preference; see `close_to_tray_enabled`.
    pub close_to_tray: bool,
    /// `GRID_ARCADE_ENABLED_KEY`: empty-session games.
    pub grid_arcade_enabled: bool,
    /// `DIFF_VIEWER_KEY`.
    pub diff_viewer: DiffViewer,
    /// `FORMAT_ON_SAVE_KEY`.
    pub format_on_save: bool,
    /// `AUTOSAVE_KEY`.
    pub autosave: bool,
    /// `CLAUDE_HOOKS_KEY`.
    pub claude_hooks: bool,
    /// `KEYBINDING_OVERRIDES_KEY`.
    pub keybinding_overrides: KeybindingOverrides,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            settings_section: SETTINGS_SECTION_DEFAULT,
            follow_up_behavior: FOLLOW_UP_BEHAVIOR_DEFAULT,
            file_tab_mode: FILE_TAB_MODE_DEFAULT,
            tab_animations_enabled: TAB_ANIMATIONS_ENABLED_DEFAULT,
            collapsed_project_rail_mode: COLLAPSED_PROJECT_RAIL_MODE_DEFAULT,
            model_controls: MODEL_CONTROLS_DEFAULT,
            composer_runner: COMPOSER_RUNNER_DEFAULT,
            notes_enabled: NOTES_ENABLED_DEFAULT,
            quick_composer_enabled: QUICK_COMPOSER_ENABLED_DEFAULT,
            quick_composer_shortcut: QUICK_COMPOSER_DEFAULT_SHORTCUT.into(),
            live_agents_enabled: LIVE_AGENTS_ENABLED_DEFAULT,
            close_to_tray: CLOSE_TO_TRAY_DEFAULT,
            grid_arcade_enabled: GRID_ARCADE_ENABLED_DEFAULT,
            diff_viewer: DIFF_VIEWER_DEFAULT,
            format_on_save: FORMAT_ON_SAVE_DEFAULT,
            autosave: AUTOSAVE_DEFAULT,
            claude_hooks: CLAUDE_HOOKS_DEFAULT,
            keybinding_overrides: KeybindingOverrides::new(),
        }
    }
}

impl Settings {
    /// Read every setting from the old localStorage values, the way each
    /// `load*` function did.
    pub fn from_local_storage(get: impl Fn(&str) -> Option<String>, platform: Platform) -> Self {
        let flag = |key: &str, default: bool| read_flag(get(key).as_deref()).unwrap_or(default);
        Self {
            settings_section: SettingsSectionId::parse(get(SECTION_KEY).as_deref()),
            follow_up_behavior: FollowUpBehavior::parse(get(FOLLOW_UP_BEHAVIOR_KEY).as_deref()),
            file_tab_mode: FileTabMode::parse(get(FILE_TAB_MODE_KEY).as_deref()),
            tab_animations_enabled: flag(
                TAB_ANIMATIONS_ENABLED_KEY,
                TAB_ANIMATIONS_ENABLED_DEFAULT,
            ),
            collapsed_project_rail_mode: CollapsedProjectRailMode::parse(
                get(COLLAPSED_PROJECT_RAIL_MODE_KEY).as_deref(),
            ),
            model_controls: parse_model_controls(
                get(MODEL_CONTROLS_KEY).as_deref(),
                get(COMPOSER_EFFORT_VISIBLE_KEY).as_deref(),
            ),
            composer_runner: flag(COMPOSER_RUNNER_KEY, COMPOSER_RUNNER_DEFAULT),
            notes_enabled: flag(NOTES_ENABLED_KEY, NOTES_ENABLED_DEFAULT),
            quick_composer_enabled: flag(
                QUICK_COMPOSER_ENABLED_KEY,
                QUICK_COMPOSER_ENABLED_DEFAULT,
            ),
            quick_composer_shortcut: parse_quick_composer_shortcut(
                get(QUICK_COMPOSER_SHORTCUT_KEY).as_deref(),
            ),
            live_agents_enabled: flag(LIVE_AGENTS_ENABLED_KEY, LIVE_AGENTS_ENABLED_DEFAULT),
            close_to_tray: flag(CLOSE_TO_TRAY_KEY, CLOSE_TO_TRAY_DEFAULT),
            grid_arcade_enabled: flag(GRID_ARCADE_ENABLED_KEY, GRID_ARCADE_ENABLED_DEFAULT),
            diff_viewer: DiffViewer::parse(get(DIFF_VIEWER_KEY).as_deref()),
            format_on_save: flag(FORMAT_ON_SAVE_KEY, FORMAT_ON_SAVE_DEFAULT),
            autosave: flag(AUTOSAVE_KEY, AUTOSAVE_DEFAULT),
            claude_hooks: flag(CLAUDE_HOOKS_KEY, CLAUDE_HOOKS_DEFAULT),
            keybinding_overrides: parse_keybinding_overrides(
                get(KEYBINDING_OVERRIDES_KEY).as_deref(),
                platform,
            ),
        }
    }

    /// `loadCloseToTray`: Close to tray is Windows-only, because no other
    /// platform installs a tray icon.
    pub fn close_to_tray_enabled(&self, platform: Platform) -> bool {
        platform.is_windows() && self.close_to_tray
    }

    /// `shortcutOwners`: chord to owning command, covering defaults, live
    /// overrides, and the Quick Composer chord.
    fn shortcut_owners(&self, platform: Platform) -> HashMap<String, String> {
        let mut owners = HashMap::new();
        for row in keybindings(platform) {
            // The Quick Composer chord is stored separately from the table.
            let chords = if row.command == QUICK_COMPOSER_COMMAND {
                vec![self.quick_composer_shortcut.clone()]
            } else {
                default_shortcuts_for(&row.command, platform)
            };
            for chord in chords {
                owners.insert(chord, row.command.clone());
            }
        }
        for (command, override_) in &self.keybinding_overrides {
            if let Some(shortcut) = override_.shortcut() {
                owners.insert(shortcut.to_string(), command.clone());
            }
        }
        owners
    }

    /// `validateKeybindingShortcut`: the canonical chord, or the message the
    /// settings row shows. No chord can be claimed twice.
    pub fn validate_keybinding_shortcut(
        &self,
        command: &str,
        shortcut: &str,
        platform: Platform,
    ) -> Result<String, String> {
        let canonical = validate_shortcut(command, shortcut)?;
        if let Some(owner) = self.shortcut_owners(platform).get(&canonical)
            && owner != command
        {
            return Err(format!("Already used by {owner}"));
        }
        Ok(canonical)
    }

    /// `saveKeybindingOverride`. An empty override restores the default. The
    /// caller removes the stored key when the map ends up empty.
    pub fn save_keybinding_override(
        &mut self,
        command: &str,
        override_: &KeybindingOverride,
        platform: Platform,
    ) -> Result<(), String> {
        if override_.is_disabled() {
            self.keybinding_overrides.insert(
                command.to_string(),
                KeybindingOverride {
                    disabled: Some(true),
                    shortcut: None,
                },
            );
        } else if let Some(shortcut) = override_.shortcut() {
            let shortcut = self.validate_keybinding_shortcut(command, shortcut, platform)?;
            self.keybinding_overrides.insert(
                command.to_string(),
                KeybindingOverride {
                    disabled: None,
                    shortcut: Some(shortcut),
                },
            );
        } else {
            self.keybinding_overrides.remove(command);
        }
        Ok(())
    }

    /// `saveQuickComposerShortcut`: ignores chords that cannot be global and
    /// applies the same conflict rules as every other row.
    pub fn save_quick_composer_shortcut(
        &mut self,
        value: &str,
        platform: Platform,
    ) -> Result<(), String> {
        if !is_global_shortcut(value) {
            return Ok(());
        }
        let shortcut =
            self.validate_keybinding_shortcut(QUICK_COMPOSER_COMMAND, value, platform)?;
        self.quick_composer_shortcut = shortcut;
        Ok(())
    }

    /// `matchCustomKeybinding`: the command a key press triggers through an override.
    pub fn match_custom_keybinding(&self, event: &ShortcutEvent) -> Option<&str> {
        self.keybinding_overrides
            .iter()
            .find(|(_, o)| {
                o.shortcut()
                    .is_some_and(|shortcut| shortcut_matches(shortcut, event))
            })
            .map(|(command, _)| command.as_str())
    }

    /// `keybindingPressed`: whether `event` triggers `command`, given whether
    /// the default binding matched.
    pub fn keybinding_pressed(
        &self,
        command: &str,
        event: &ShortcutEvent,
        default_match: bool,
    ) -> bool {
        match self.keybinding_overrides.get(command) {
            Some(o) if o.is_disabled() => false,
            Some(o) => match o.shortcut() {
                Some(shortcut) => shortcut_matches(shortcut, event),
                None => default_match,
            },
            None => default_match,
        }
    }

    /// `keybindingShortcutLabel`: `None` when the user disabled the command.
    pub fn keybinding_shortcut_label(
        &self,
        command: &str,
        fallback: &str,
        platform: Platform,
    ) -> Option<String> {
        let o = self.keybinding_overrides.get(command);
        if o.is_some_and(KeybindingOverride::is_disabled) {
            return None;
        }
        Some(label_for(o, fallback, platform))
    }

    /// `keybindingShortcutTokens`: `aria-keyshortcuts` form.
    pub fn keybinding_shortcut_tokens(&self, command: &str, fallback: &str) -> Option<String> {
        let o = self.keybinding_overrides.get(command);
        if o.is_some_and(KeybindingOverride::is_disabled) {
            return None;
        }
        Some(match o.and_then(KeybindingOverride::shortcut) {
            Some(shortcut) => shortcut_tokens(shortcut),
            None => fallback.to_string(),
        })
    }

    /// `currentKeybindings`: the table with overrides applied.
    pub fn current_keybindings(&self, platform: Platform) -> Vec<KeybindingRow> {
        keybindings(platform)
            .into_iter()
            .map(|row| {
                let keys = if row.command == QUICK_COMPOSER_COMMAND {
                    if self.quick_composer_enabled {
                        quick_composer_shortcut_label(&self.quick_composer_shortcut, platform)
                    } else {
                        "Disabled".into()
                    }
                } else {
                    label_for(
                        self.keybinding_overrides.get(&row.command),
                        &row.keys,
                        platform,
                    )
                };
                KeybindingRow { keys, ..row }
            })
            .collect()
    }
}

/// `shortcutMatches`.
pub fn shortcut_matches(shortcut: &str, event: &ShortcutEvent) -> bool {
    shortcut_from_key_event(event).as_deref() == Some(shortcut)
}

/// `filterKeybindings`.
pub fn filter_keybindings(rows: &[KeybindingRow], query: &str) -> Vec<KeybindingRow> {
    let needle = js::trim(query).to_lowercase();
    if needle.is_empty() {
        return rows.to_vec();
    }
    rows.iter()
        .filter(|row| {
            row.command.to_lowercase().contains(&needle)
                || row.keys.to_lowercase().contains(&needle)
                || row.when.to_lowercase().contains(&needle)
        })
        .cloned()
        .collect()
}

/// Every stored preference in one value: app behavior, appearance, model
/// choices, and per-project provider overrides. This is the shape a settings
/// file can hold once the old localStorage values are imported.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    pub settings: Settings,
    pub appearance: crate::appearance::AppearanceSettings,
    pub models: crate::models::ModelPrefs,
    /// `PROJECT_PROVIDER_SETTINGS_KEY`.
    pub project_providers: crate::project_providers::ProjectProviders,
}

impl AppSettings {
    /// Read every old localStorage value. `get` returns the stored string
    /// for a key, or `None` when the key was never written.
    pub fn from_local_storage(get: impl Fn(&str) -> Option<String>, platform: Platform) -> Self {
        Self {
            settings: Settings::from_local_storage(&get, platform),
            appearance: crate::appearance::AppearanceSettings::from_local_storage(&get, platform),
            models: crate::models::ModelPrefs::from_local_storage(&get),
            project_providers: crate::project_providers::ProjectProviders::parse(
                get(crate::project_providers::PROJECT_PROVIDER_SETTINGS_KEY).as_deref(),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shortcut::Modifiers;

    const MAC: Platform = Platform::Mac;

    fn stored(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |key| map.get(key).cloned()
    }

    fn key(code: &str, modifiers: Modifiers) -> ShortcutEvent {
        ShortcutEvent {
            code: code.into(),
            modifiers,
        }
    }

    fn meta() -> Modifiers {
        Modifiers {
            meta_key: true,
            ..Modifiers::default()
        }
    }

    fn meta_shift() -> Modifiers {
        Modifiers {
            meta_key: true,
            shift_key: true,
            ..Modifiers::default()
        }
    }

    fn shortcut(value: &str) -> KeybindingOverride {
        KeybindingOverride {
            disabled: None,
            shortcut: Some(value.into()),
        }
    }

    // storage flags (settings.flags.test.ts)
    #[test]
    fn reads_flags_like_read_flag() {
        for (raw, expected) in [
            ("1", true),
            ("true", true),
            ("0", false),
            ("false", false),
            ("", false),
            ("TRUE", false),
            (" true ", false),
            ("invalid", false),
        ] {
            assert_eq!(read_flag(Some(raw)), Some(expected), "{raw:?}");
        }
        assert_eq!(read_flag(None), None);
        assert_eq!(write_flag(true), "1");
        assert_eq!(write_flag(false), "0");
    }

    #[test]
    fn imports_every_group_of_settings() {
        let all = AppSettings::from_local_storage(
            stored(&[
                (FOLLOW_UP_BEHAVIOR_KEY, "queue"),
                (crate::appearance::THEME_HUE_KEY, "120"),
                (
                    crate::models::LAST_MODEL_KEY,
                    r#"{"harness":"grok","model":"grok:grok-4.6"}"#,
                ),
                (
                    crate::project_providers::PROJECT_PROVIDER_SETTINGS_KEY,
                    r#"{"/r":{"hidden":["pi"]}}"#,
                ),
            ]),
            MAC,
        );
        assert_eq!(all.settings.follow_up_behavior, FollowUpBehavior::Queue);
        assert_eq!(all.appearance.theme_hue, 120);
        assert_eq!(all.models.last_model.unwrap().model, "grok:grok-4.6");
        assert!(
            all.project_providers
                .is_provider_hidden(Some("/r"), crate::harness::HarnessId::Pi)
        );
        let json = serde_json::to_string(&AppSettings::default()).unwrap();
        assert_eq!(
            serde_json::from_str::<AppSettings>(&json).unwrap(),
            AppSettings::default()
        );
    }

    #[test]
    fn every_flag_uses_its_default_when_unset() {
        let settings = Settings::from_local_storage(stored(&[]), MAC);
        assert_eq!(settings, Settings::default());
        assert!(!settings.tab_animations_enabled);
        assert!(settings.composer_runner);
        assert!(settings.notes_enabled);
        assert!(settings.live_agents_enabled);
        assert!(settings.close_to_tray);
        assert!(settings.grid_arcade_enabled);
        assert!(settings.claude_hooks);
        assert!(settings.format_on_save);
        assert!(!settings.autosave);
    }

    #[test]
    fn reads_stored_switches() {
        let settings = Settings::from_local_storage(
            stored(&[
                (TAB_ANIMATIONS_ENABLED_KEY, "1"),
                (COMPOSER_RUNNER_KEY, "0"),
                (NOTES_ENABLED_KEY, "0"),
                (LIVE_AGENTS_ENABLED_KEY, "false"),
                (GRID_ARCADE_ENABLED_KEY, "0"),
                (FORMAT_ON_SAVE_KEY, "0"),
                (AUTOSAVE_KEY, "true"),
                (CLAUDE_HOOKS_KEY, "nope"),
            ]),
            MAC,
        );
        assert!(settings.tab_animations_enabled);
        assert!(!settings.composer_runner);
        assert!(!settings.notes_enabled);
        assert!(!settings.live_agents_enabled);
        assert!(!settings.grid_arcade_enabled);
        assert!(!settings.format_on_save);
        assert!(settings.autosave);
        assert!(!settings.claude_hooks);
    }

    #[test]
    fn disables_close_to_tray_outside_windows() {
        let settings = Settings::from_local_storage(stored(&[(CLOSE_TO_TRAY_KEY, "1")]), MAC);
        assert!(!settings.close_to_tray_enabled(Platform::Mac));
        assert!(!settings.close_to_tray_enabled(Platform::Linux));
        assert!(settings.close_to_tray_enabled(Platform::Windows));
    }

    // follow-up behavior, file tab mode, diff viewer, rail mode
    #[test]
    fn enum_settings_default_persist_and_ignore_unknown_values() {
        assert_eq!(FollowUpBehavior::parse(None), FollowUpBehavior::Steer);
        assert_eq!(
            FollowUpBehavior::parse(Some("queue")),
            FollowUpBehavior::Queue
        );
        assert_eq!(
            FollowUpBehavior::parse(Some("interrupt")),
            FollowUpBehavior::Steer
        );
        assert_eq!(FileTabMode::parse(None), FileTabMode::Pane);
        assert_eq!(
            FileTabMode::parse(Some("workspace")),
            FileTabMode::Workspace
        );
        assert_eq!(FileTabMode::parse(Some("tabs")), FileTabMode::Pane);
        assert_eq!(DiffViewer::parse(None), DiffViewer::Editor);
        assert_eq!(DiffViewer::parse(Some("unified")), DiffViewer::Unified);
        assert_eq!(DiffViewer::parse(Some("split")), DiffViewer::Editor);
        assert_eq!(
            CollapsedProjectRailMode::parse(Some("hidden")),
            CollapsedProjectRailMode::Hidden
        );
        assert_eq!(
            CollapsedProjectRailMode::parse(Some("gone")),
            CollapsedProjectRailMode::Compact
        );
        assert_eq!(
            SettingsSectionId::parse(Some("mcp")),
            SettingsSectionId::Mcp
        );
        assert_eq!(
            SettingsSectionId::parse(Some("nope")),
            SettingsSectionId::General
        );
        assert_eq!(
            serde_json::to_string(&FollowUpBehavior::Queue).unwrap(),
            "\"queue\""
        );
    }

    // model controls setting
    #[test]
    fn model_controls_default_persist_and_migrate() {
        assert_eq!(parse_model_controls(None, None), ModelControls::Menu);
        assert_eq!(
            parse_model_controls(Some("beside"), None),
            ModelControls::Beside
        );
        assert_eq!(
            parse_model_controls(Some("sideways"), None),
            ModelControls::Menu
        );
        assert_eq!(parse_model_controls(None, Some("1")), ModelControls::Beside);
        assert_eq!(
            parse_model_controls(None, Some("true")),
            ModelControls::Beside
        );
        assert_eq!(parse_model_controls(None, Some("0")), ModelControls::Menu);
        assert_eq!(
            parse_model_controls(Some("menu"), Some("1")),
            ModelControls::Menu
        );
    }

    // keybinding overrides
    #[test]
    fn persists_a_custom_shortcut_and_matches_only_that_combination() {
        let mut settings = Settings::default();
        settings
            .save_keybinding_override("App: Search", &shortcut("Command+Shift+KeyM"), MAC)
            .unwrap();
        assert_eq!(
            serde_json::to_string(&settings.keybinding_overrides).unwrap(),
            r#"{"App: Search":{"shortcut":"Command+Shift+KeyM"}}"#
        );
        assert!(settings.keybinding_pressed("App: Search", &key("KeyM", meta_shift()), true));
        assert!(!settings.keybinding_pressed("App: Search", &key("KeyK", meta()), true));
        assert_eq!(
            settings.match_custom_keybinding(&key("KeyM", meta_shift())),
            Some("App: Search")
        );
        settings
            .save_keybinding_override("App: Search", &KeybindingOverride::default(), MAC)
            .unwrap();
        assert!(settings.keybinding_overrides.is_empty());
    }

    #[test]
    fn disables_a_shortcut_and_restores_the_default() {
        let mut settings = Settings::default();
        settings
            .save_keybinding_override(
                "App: Search",
                &KeybindingOverride {
                    disabled: Some(true),
                    shortcut: None,
                },
                MAC,
            )
            .unwrap();
        assert_eq!(
            serde_json::to_value(&settings.keybinding_overrides).unwrap(),
            serde_json::json!({ "App: Search": { "disabled": true } })
        );
        assert!(!settings.keybinding_pressed("App: Search", &key("KeyK", meta()), true));
        assert_eq!(
            settings.keybinding_shortcut_label("App: Search", "⌘K", MAC),
            None
        );
        assert_eq!(
            settings
                .keybinding_shortcut_label("App: Go to File", "⌘P", MAC)
                .as_deref(),
            Some("⌘P")
        );
    }

    #[test]
    fn rejects_a_shortcut_already_used_by_another_command() {
        let mut settings = Settings::default();
        settings
            .save_keybinding_override("App: Search", &shortcut("Command+KeyY"), MAC)
            .unwrap();
        assert_eq!(
            settings.save_keybinding_override("App: Go to File", &shortcut("Command+KeyY"), MAC),
            Err("Already used by App: Search".into())
        );
    }

    #[test]
    fn rejects_a_shortcut_that_shadows_another_commands_default() {
        for (platform, primary) in [(Platform::Mac, "Command"), (Platform::Windows, "Control")] {
            let mut settings = Settings::default();
            assert_eq!(
                settings.save_keybinding_override(
                    "App: Search",
                    &shortcut(&format!("{primary}+KeyP")),
                    platform
                ),
                Err("Already used by App: Go to File".into())
            );
            assert_eq!(
                settings.save_keybinding_override("Tab: New", &shortcut("Control+Tab"), platform),
                Err("Already used by Tab: Cycle Next".into())
            );
        }
    }

    #[test]
    fn protects_every_chord_in_the_grouped_tab_activation_range() {
        let mut settings = Settings::default();
        for digit in [1, 4, 8] {
            assert_eq!(
                settings.save_keybinding_override(
                    "App: Search",
                    &shortcut(&format!("Command+Digit{digit}")),
                    MAC
                ),
                Err("Already used by Tab: Activate 1–8".into())
            );
        }
    }

    #[test]
    fn rejects_a_chord_the_command_cannot_use_and_invalid_chords() {
        let mut settings = Settings::default();
        let error = settings
            .save_keybinding_override(ACTIVATE_RANGE_COMMAND, &shortcut("Control+KeyM"), MAC)
            .unwrap_err();
        assert!(error.contains("needs a number key"));
        let error = settings
            .save_keybinding_override("App: Search", &shortcut("KeyK"), MAC)
            .unwrap_err();
        assert!(error.contains("not a valid shortcut"));
        settings
            .save_keybinding_override(ACTIVATE_RANGE_COMMAND, &shortcut("Control+Digit3"), MAC)
            .unwrap();
    }

    #[test]
    fn normalises_modifier_order_when_reading_stored_shortcuts() {
        assert_eq!(
            parse_keybinding_overrides(
                Some(r#"{"App: Search":{"shortcut":"Shift+Command+KeyM"}}"#),
                MAC
            ),
            KeybindingOverrides::from([(
                "App: Search".to_string(),
                shortcut("Command+Shift+KeyM")
            )])
        );
    }

    #[test]
    fn ignores_malformed_unknown_and_invalid_stored_overrides() {
        assert_eq!(
            parse_keybinding_overrides(
                Some(
                    r#"{"Unknown: Command":{"disabled":true},"App: Search":{"shortcut":"KeyK"},"Tab: New":{"shortcut":"Command+KeyT"}}"#
                ),
                MAC
            ),
            KeybindingOverrides::from([("Tab: New".to_string(), shortcut("Command+KeyT"))])
        );
        assert!(parse_keybinding_overrides(Some("not-json"), MAC).is_empty());
        assert!(parse_keybinding_overrides(None, MAC).is_empty());
        assert!(
            parse_keybinding_overrides(
                Some(r#"{"App: Quick Composer":{"disabled":true}}"#),
                Platform::Windows
            )
            .is_empty()
        );
    }

    // quick composer shortcut setting
    #[test]
    fn defaults_to_the_existing_shortcut_and_persists_a_custom_binding() {
        let mut settings = Settings::from_local_storage(stored(&[]), MAC);
        assert_eq!(settings.quick_composer_shortcut, "Command+Shift+Space");
        settings
            .save_quick_composer_shortcut("Command+Option+KeyK", MAC)
            .unwrap();
        assert_eq!(settings.quick_composer_shortcut, "Command+Option+KeyK");
        assert_eq!(
            settings.save_quick_composer_shortcut("Command+KeyK", MAC),
            Err("Already used by App: Search".into())
        );
        settings
            .save_quick_composer_shortcut("Option+KeyK", MAC)
            .unwrap();
        assert_eq!(settings.quick_composer_shortcut, "Command+Option+KeyK");
    }

    #[test]
    fn ignores_malformed_stored_bindings() {
        assert_eq!(
            parse_quick_composer_shortcut(Some("Shift+Space")),
            "Command+Shift+Space"
        );
    }

    // workspace navigation keybindings
    #[test]
    fn keeps_separate_shortcuts_for_the_project_rail_and_session_sidebar() {
        let rows = keybindings(MAC);
        let find = |command: &str| {
            rows.iter()
                .find(|row| row.command == command)
                .unwrap()
                .clone()
        };
        assert_eq!(find("App: Toggle Sidebar").keys, "⌘B");
        assert_eq!(find("App: Toggle Session Sidebar").keys, "⌘⇧B");
        assert_eq!(find("App: Command Palette").keys, "⌘⇧P");
        assert_eq!(find("View: Reload").keys, "⌘⇧R");
        assert_eq!(
            find("Composer: Toggle Workspace"),
            KeybindingRow {
                command: "Composer: Toggle Workspace".into(),
                keys: "⌘⇧G".into(),
                when: "Draft session composer".into(),
            }
        );
        assert_eq!(find("Session: Previous in Current Tab").keys, "⌘↑");
        assert_eq!(
            keybindings(Platform::Windows)
                .iter()
                .find(|r| r.command == "Tab: Cycle Previous")
                .unwrap()
                .keys,
            "Ctrl+Shift+Tab"
        );
    }

    #[test]
    fn documents_session_and_project_cycling_in_the_shortcut_list() {
        let rows: Vec<KeybindingRow> = keybindings(MAC)
            .into_iter()
            .filter(|row| {
                [
                    "Session: Previous",
                    "Session: Next",
                    "Project: Previous",
                    "Project: Next",
                ]
                .contains(&row.command.as_str())
            })
            .collect();
        let commands: Vec<&str> = rows.iter().map(|row| row.command.as_str()).collect();
        assert_eq!(
            commands,
            [
                "Session: Previous",
                "Session: Next",
                "Project: Previous",
                "Project: Next"
            ]
        );
        assert!(
            rows.iter()
                .all(|row| row.when == "!overlay && (!textFocus || emptyComposer)")
        );
    }

    #[test]
    fn expands_default_chords_from_the_table() {
        assert_eq!(
            default_shortcuts_for("App: Settings", MAC),
            ["Command+Comma"]
        );
        assert_eq!(
            default_shortcuts_for("View: Zoom In", Platform::Linux),
            ["Control+Equal"]
        );
        assert_eq!(
            default_shortcuts_for("Tab: Cycle Previous", MAC),
            ["Control+Shift+Tab"]
        );
        assert_eq!(
            default_shortcuts_for("Pane: Focus Left", MAC),
            ["Command+Option+ArrowLeft"]
        );
        assert_eq!(default_shortcuts_for(ACTIVATE_RANGE_COMMAND, MAC).len(), 8);
        assert!(default_shortcuts_for("Nope", MAC).is_empty());
        // The Quick Composer row exists only on macOS.
        assert_eq!(
            keybindings(MAC).len(),
            keybindings(Platform::Windows).len() + 1
        );
    }

    #[test]
    fn shows_overrides_and_quick_composer_state_in_the_table() {
        let mut settings = Settings::default();
        settings
            .save_keybinding_override("App: Search", &shortcut("Command+Shift+KeyM"), MAC)
            .unwrap();
        settings.quick_composer_enabled = false;
        let rows = settings.current_keybindings(MAC);
        let find = |command: &str| {
            rows.iter()
                .find(|row| row.command == command)
                .unwrap()
                .keys
                .clone()
        };
        assert_eq!(find("App: Search"), "⌘⇧M");
        assert_eq!(find(QUICK_COMPOSER_COMMAND), "Disabled");
        assert_eq!(find("App: Go to File"), "⌘P");
        assert_eq!(filter_keybindings(&rows, "zoom").len(), 3);
        assert_eq!(filter_keybindings(&rows, " ").len(), rows.len());
        assert_eq!(
            settings
                .keybinding_shortcut_tokens("App: Search", "Meta+K")
                .as_deref(),
            Some("Meta+Shift+M")
        );
    }

    // settings navigation
    #[test]
    fn lists_every_section_under_exactly_one_rail_group() {
        let groups = settings_sections_by_group();
        let labels: Vec<&str> = groups.iter().map(|group| group.label).collect();
        assert_eq!(labels, ["App", "Agents", "Workspace"]);
        let ids: Vec<&str> = groups
            .iter()
            .flat_map(|group| group.sections.iter().map(|section| section.id.as_str()))
            .collect();
        assert_eq!(
            ids,
            [
                "general",
                "connections",
                "appearance",
                "keybindings",
                "chat",
                "providers",
                "mcp",
                "skills",
                "inbox",
                "archive",
                "worktrees"
            ]
        );
    }

    #[test]
    fn points_every_indexed_setting_at_a_real_section() {
        for platform in [Platform::Mac, Platform::Windows, Platform::Linux] {
            for entry in settings_index(platform) {
                assert!(
                    SETTINGS_SECTIONS
                        .iter()
                        .any(|section| section.id == entry.section),
                    "{}",
                    entry.id
                );
            }
        }
    }

    // settings search
    #[test]
    fn returns_nothing_for_an_empty_query() {
        assert!(search_settings("   ", 8, Platform::Linux).is_empty());
    }

    #[test]
    fn ranks_label_matches_over_keyword_matches_and_pages_last() {
        let labels: Vec<&str> = search_settings("glass", 8, Platform::Linux)
            .iter()
            .map(|result| result.label)
            .collect();
        assert_eq!(
            labels,
            [
                "Main pane glass",
                "Blur radius",
                "Main pane opacity",
                "Sidebar opacity",
                "Appearance"
            ]
        );
    }

    #[test]
    fn finds_a_setting_by_a_word_that_is_not_in_its_label() {
        let first = search_settings("steer", 8, Platform::Linux)[0].clone();
        assert_eq!(first.section, SettingsSectionId::Chat);
        assert_eq!(first.section_label, "Chat");
        assert_eq!(first.setting_id, Some("follow-up"));
        assert_eq!(first.label, "Follow-up behavior");
        let first = search_settings("prettier", 8, Platform::Linux)[0].clone();
        assert_eq!(first.setting_id, Some("format-on-save"));
    }

    #[test]
    fn returns_a_whole_page_with_no_setting_id() {
        assert_eq!(
            search_settings("skills", 8, Platform::Linux),
            [SettingsSearchResult {
                section: SettingsSectionId::Skills,
                section_label: "Skills",
                setting_id: None,
                label: "Skills",
            }]
        );
    }

    #[test]
    fn caps_the_result_list() {
        assert_eq!(search_settings("e", 4, Platform::Linux).len(), 4);
    }
}
