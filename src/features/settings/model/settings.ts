import {
  ALT,
  IS_MAC,
  IS_WIN,
  MOD,
  SHIFT,
} from "../../../platform/tauri/platform";
import {
  canonicalShortcut,
  isGlobalShortcut,
  QUICK_COMPOSER_DEFAULT_SHORTCUT,
  quickComposerShortcutLabel,
  shortcutFromKeyEvent,
} from "../../quick-composer/model/quickComposerShortcut";
import { readFlag, writeFlag } from "./storageFlags";

const SECTION_KEY = "monocode.settingsSection";

export type SettingsSectionId =
  | "general"
  | "appearance"
  | "keybindings"
  | "chat"
  | "providers"
  | "skills"
  | "inbox"
  | "worktrees"
  | "archive";

/** Rail buckets. Sections list in order under their group label. */
export type SettingsGroupId = "app" | "agents" | "workspace";

export const SETTINGS_GROUPS: { id: SettingsGroupId; label: string }[] = [
  { id: "app", label: "App" },
  { id: "agents", label: "Agents" },
  { id: "workspace", label: "Workspace" },
];

export type SettingsSection = {
  id: SettingsSectionId;
  group: SettingsGroupId;
  label: string;
  description: string;
  /** Extra words search matches the section on, beyond its label. */
  keywords?: string;
};

export const SETTINGS_SECTIONS: SettingsSection[] = [
  {
    id: "general",
    group: "app",
    label: "General",
    description:
      "The build you are running, how MonoCode reaches you, and the panels it shows.",
    keywords: "version update sounds notifications notes rail",
  },
  {
    id: "appearance",
    group: "app",
    label: "Appearance",
    description:
      "Theme, tint, translucency, workspace layout, and conversation backgrounds.",
    keywords:
      "theme dark light color accent glass blur zoom scale wallpaper rail sidebar",
  },
  {
    id: "keybindings",
    group: "app",
    label: "Keybindings",
    description:
      "Every shortcut the workspace handles, from the app menu and the key handler.",
    keywords: "shortcut hotkey keyboard binding",
  },
  {
    id: "chat",
    group: "agents",
    label: "Chat",
    description:
      "How transcripts read, what the composer does with a follow-up, how files save, and how diffs open.",
    keywords:
      "transcript composer prompt message diff review layout format save editor",
  },
  {
    id: "providers",
    group: "agents",
    label: "Providers",
    description:
      "Provider accounts, agent CLIs MonoCode can drive, and the model new sessions start with.",
    keywords:
      "account sign in login model harness claude codex gemini cli default hooks",
  },
  {
    id: "skills",
    group: "agents",
    label: "Skills",
    description:
      "Discover and manage file skills from project, personal, and harness folders.",
    keywords: "skill instructions prompt",
  },
  {
    id: "inbox",
    group: "workspace",
    label: "Inbox",
    description:
      "Manage Inbox services and notification preferences for each project.",
    keywords:
      "github gitlab linear jira atlassian azure devops connect token integration",
  },
  {
    id: "archive",
    group: "workspace",
    label: "Archive",
    description: "Projects and conversations you have archived.",
    keywords: "archived restore delete hidden",
  },
  {
    id: "worktrees",
    group: "workspace",
    label: "Worktrees",
    description: "Manage additional worktrees for each project.",
    keywords: "git branch worktree working copy project create delete",
  },
];

export function settingsSectionsByGroup(): {
  id: SettingsGroupId;
  label: string;
  sections: SettingsSection[];
}[] {
  return SETTINGS_GROUPS.map((group) => ({
    ...group,
    sections: SETTINGS_SECTIONS.filter((section) => section.group === group.id),
  })).filter((group) => group.sections.length > 0);
}

/**
 * One searchable control. `id` is the row's `data-setting-id` in SettingsView,
 * which is also what Settings scrolls to when it opens on an anchor.
 */
export type SettingsEntry = {
  id: string;
  section: SettingsSectionId;
  label: string;
  keywords?: string;
};

export const SETTINGS_INDEX: SettingsEntry[] = [
  {
    id: "project-worktrees",
    section: "worktrees",
    label: "Project worktrees",
    keywords: "git branch working copy create delete manage",
  },
  {
    id: "update",
    section: "general",
    label: "Version",
    keywords: "update upgrade release what's new build changelog",
  },
  {
    id: "sounds",
    section: "general",
    label: "Sounds",
    keywords: "audio cue chime mute volume",
  },
  {
    id: "notifications",
    section: "general",
    label: "Notifications",
    keywords: "notify alert toast permission reminder background",
  },
  {
    id: "notes",
    section: "general",
    label: "Notes",
    keywords: "notebook markdown rail scratchpad",
  },
  ...(IS_MAC
    ? [
        {
          id: "quick-composer",
          section: "general" as const,
          label: "Quick composer",
          keywords: "spotlight global shortcut hotkey floating prompt anywhere",
        },
      ]
    : []),
  {
    id: "working-agents",
    section: "general",
    label: "Working agents",
    keywords: "live running sessions rail card",
  },
  {
    id: "file-tabs",
    section: "general",
    label: "File tabs",
    keywords: "editor open top workspace normal session pane beside chat",
  },
  {
    id: "tab-animations",
    section: "general",
    label: "Tab animations",
    keywords: "motion open close resize transition",
  },
  ...(IS_WIN
    ? [
        {
          id: "close-to-tray",
          section: "general" as const,
          label: "Close to tray",
          keywords: "minimize background quit exit window taskbar windows",
        },
      ]
    : []),
  {
    id: "theme",
    section: "appearance",
    label: "Theme",
    keywords: "dark light system appearance mode",
  },
  {
    id: "accent-color",
    section: "appearance",
    label: "Accent color",
    keywords: "highlight bubble send button tint",
  },
  {
    id: "hue",
    section: "appearance",
    label: "Hue",
    keywords: "tint color chrome",
  },
  {
    id: "saturation",
    section: "appearance",
    label: "Saturation",
    keywords: "tint color neutral grey gray",
  },
  {
    id: "dark-lightness",
    section: "appearance",
    label: "Dark-mode lightness",
    keywords: "black brightness contrast background",
  },
  {
    id: "sidebar-opacity",
    section: "appearance",
    label: "Sidebar opacity",
    keywords: "glass translucent transparency vibrancy",
  },
  {
    id: "blur",
    section: "appearance",
    label: "Blur radius",
    keywords: "glass translucent vibrancy backdrop",
  },
  {
    id: "main-pane-glass",
    section: "appearance",
    label: "Main pane glass",
    keywords: "translucent transparency body window",
  },
  {
    id: "interface-scale",
    section: "appearance",
    label: "Interface scale",
    keywords: "zoom font size bigger smaller ui",
  },
  {
    id: "collapsed-project-rail",
    section: "appearance",
    label: "Collapsed project rail",
    keywords: "sidebar compact icons hidden navigation layout",
  },
  {
    id: "show-excluded-files",
    section: "appearance",
    label: "Show excluded files",
    keywords: "explorer gitignore ignored hidden files tree",
  },
  {
    id: "chat-background",
    section: "appearance",
    label: "Chat background",
    keywords: "wallpaper image picture opacity backdrop",
  },
  {
    id: "transcript-layout",
    section: "chat",
    label: "Transcript layout",
    keywords: "full width chat bubble message",
  },
  {
    id: "anchor-prompts",
    section: "chat",
    label: "Anchor prompts to top",
    keywords: "scroll position sticky message",
  },
  {
    id: "follow-up",
    section: "chat",
    label: "Follow-up behavior",
    keywords: "queue steer interrupt send while running",
  },
  {
    id: "model-controls",
    section: "chat",
    label: "Model controls",
    keywords:
      "effort thinking reasoning fast service tier model picker composer",
  },
  {
    id: "composer-mascot",
    section: "chat",
    label: "Composer mascot",
    keywords: "runner animation coin fun",
  },
  {
    id: "format-on-save",
    section: "chat",
    label: "Format on save",
    keywords: "prettier quotes editor save format",
  },
  {
    id: "diff-view",
    section: "chat",
    label: "Diff view",
    keywords: "unified editor review changes working tree",
  },
  {
    id: "empty-session-games",
    section: "chat",
    label: "Empty session games",
    keywords: "pacman snake arcade grid fun",
  },
  {
    id: "provider-accounts",
    section: "providers",
    label: "Provider accounts",
    keywords: "account sign in login rename remove delete credentials profile",
  },
  {
    id: "claude-hooks",
    section: "providers",
    label: "Claude Code hooks",
    keywords: "pretooluse settings.json block command notification",
  },
  {
    id: "project-notifications",
    section: "inbox",
    label: "Project notifications",
    keywords: "mute resume sounds banners reminders categories",
  },
  {
    id: "github",
    section: "inbox",
    label: "GitHub",
    keywords: "gh cli connect pull request sign in",
  },
  {
    id: "gitlab",
    section: "inbox",
    label: "GitLab",
    keywords: "token self-managed merge request connect",
  },
  {
    id: "azuredevops",
    section: "inbox",
    label: "ADO",
    keywords: "azure devops boards repos pull request pat organization connect",
  },
  {
    id: "jira",
    section: "inbox",
    label: "Jira",
    keywords: "atlassian cloud site email api token issues projects connect",
  },
  {
    id: "linear",
    section: "inbox",
    label: "Linear",
    keywords: "api key issues teams connect",
  },
  {
    id: "show-archived",
    section: "archive",
    label: "Show archived in the sidebar",
    keywords: "hidden conversations list",
  },
];

export type SettingsSearchResult = {
  section: SettingsSectionId;
  sectionLabel: string;
  /** Row to scroll to, or `null` when the whole section matched. */
  settingId: string | null;
  label: string;
};

/** Ranks a label/keyword pair against a lowercased needle; `null` means no match. */
function matchScore(
  needle: string,
  label: string,
  keywords?: string,
): number | null {
  const lower = label.toLowerCase();
  if (lower.startsWith(needle)) return 0;
  if (lower.includes(needle)) return 1;
  if (keywords?.toLowerCase().includes(needle)) return 2;
  return null;
}

/** Individual settings first, then whole sections, so a row wins its own name. */
export function searchSettings(
  query: string,
  limit = 8,
): SettingsSearchResult[] {
  const needle = query.trim().toLowerCase();
  if (!needle) return [];
  const scored: { score: number; result: SettingsSearchResult }[] = [];

  for (const entry of SETTINGS_INDEX) {
    const score = matchScore(needle, entry.label, entry.keywords);
    if (score == null) continue;
    scored.push({
      score,
      result: {
        section: entry.section,
        sectionLabel: settingsSectionLabel(entry.section),
        settingId: entry.id,
        label: entry.label,
      },
    });
  }

  for (const section of SETTINGS_SECTIONS) {
    const score = matchScore(
      needle,
      section.label,
      `${section.description} ${section.keywords ?? ""}`,
    );
    if (score == null) continue;
    scored.push({
      score: score + 0.5,
      result: {
        section: section.id,
        sectionLabel: section.label,
        settingId: null,
        label: section.label,
      },
    });
  }

  return scored
    .sort(
      (a, b) =>
        a.score - b.score || a.result.label.localeCompare(b.result.label),
    )
    .slice(0, limit)
    .map((item) => item.result);
}

export const SETTINGS_SECTION_DEFAULT: SettingsSectionId = "general";

export function isSettingsSectionId(
  value: unknown,
): value is SettingsSectionId {
  return SETTINGS_SECTIONS.some((section) => section.id === value);
}

export function settingsSectionLabel(id: SettingsSectionId): string {
  return (
    SETTINGS_SECTIONS.find((section) => section.id === id)?.label ?? "General"
  );
}

export function settingsSectionDescription(id: SettingsSectionId): string {
  return (
    SETTINGS_SECTIONS.find((section) => section.id === id)?.description ?? ""
  );
}

export function loadSettingsSection(): SettingsSectionId {
  try {
    const raw = localStorage.getItem(SECTION_KEY);
    return isSettingsSectionId(raw) ? raw : SETTINGS_SECTION_DEFAULT;
  } catch {
    return SETTINGS_SECTION_DEFAULT;
  }
}

export function saveSettingsSection(id: SettingsSectionId) {
  try {
    localStorage.setItem(SECTION_KEY, id);
  } catch {
    // private mode / quota
  }
}

const COMPOSER_RUNNER_KEY = "monocode.composerRunner";

const FOLLOW_UP_BEHAVIOR_KEY = "monocode.followUpBehavior";

const COMPOSER_EFFORT_VISIBLE_KEY = "monocode.composerEffortVisible";

const MODEL_CONTROLS_KEY = "monocode.modelControls";

const FILE_TAB_MODE_KEY = "monocode.fileTabMode";

const TAB_ANIMATIONS_ENABLED_KEY = "monocode.tabAnimationsEnabled";

const COLLAPSED_PROJECT_RAIL_MODE_KEY = "monocode.collapsedProjectRailMode";

export type FollowUpBehavior = "steer" | "queue";

export const FOLLOW_UP_BEHAVIOR_DEFAULT: FollowUpBehavior = "steer";

export function loadFollowUpBehavior(): FollowUpBehavior {
  try {
    const raw = localStorage.getItem(FOLLOW_UP_BEHAVIOR_KEY);
    return raw === "queue" || raw === "steer"
      ? raw
      : FOLLOW_UP_BEHAVIOR_DEFAULT;
  } catch {
    return FOLLOW_UP_BEHAVIOR_DEFAULT;
  }
}

export function saveFollowUpBehavior(value: FollowUpBehavior) {
  try {
    localStorage.setItem(FOLLOW_UP_BEHAVIOR_KEY, value);
  } catch {
    // private mode / quota
  }
}

export type FileTabMode = "pane" | "workspace";

export const FILE_TAB_MODE_DEFAULT: FileTabMode = "pane";

/** Choose whether an ordinary file joins the active pane or gets a top tab. */
export function loadFileTabMode(): FileTabMode {
  try {
    const raw = localStorage.getItem(FILE_TAB_MODE_KEY);
    return raw === "pane" || raw === "workspace" ? raw : FILE_TAB_MODE_DEFAULT;
  } catch {
    return FILE_TAB_MODE_DEFAULT;
  }
}

export function saveFileTabMode(value: FileTabMode) {
  try {
    localStorage.setItem(FILE_TAB_MODE_KEY, value);
  } catch {
    // private mode / quota
  }
}

export const TAB_ANIMATIONS_ENABLED_DEFAULT = false;

export function loadTabAnimationsEnabled(): boolean {
  return readFlag(TAB_ANIMATIONS_ENABLED_KEY) ?? TAB_ANIMATIONS_ENABLED_DEFAULT;
}

export function saveTabAnimationsEnabled(value: boolean) {
  writeFlag(TAB_ANIMATIONS_ENABLED_KEY, value);
}

export type CollapsedProjectRailMode = "compact" | "hidden";

export const COLLAPSED_PROJECT_RAIL_MODE_DEFAULT: CollapsedProjectRailMode =
  "compact";

export const COLLAPSED_PROJECT_RAIL_MODE_CHANGE_EVENT =
  "monocode:collapsed-project-rail-mode-change";

export function loadCollapsedProjectRailMode(): CollapsedProjectRailMode {
  try {
    const raw = localStorage.getItem(COLLAPSED_PROJECT_RAIL_MODE_KEY);
    return raw === "compact" || raw === "hidden"
      ? raw
      : COLLAPSED_PROJECT_RAIL_MODE_DEFAULT;
  } catch {
    return COLLAPSED_PROJECT_RAIL_MODE_DEFAULT;
  }
}

export function saveCollapsedProjectRailMode(value: CollapsedProjectRailMode) {
  try {
    localStorage.setItem(COLLAPSED_PROJECT_RAIL_MODE_KEY, value);
  } catch {
    // private mode / quota
  }
  if (typeof window === "undefined") return;
  window.dispatchEvent(
    new CustomEvent<CollapsedProjectRailMode>(
      COLLAPSED_PROJECT_RAIL_MODE_CHANGE_EVENT,
      { detail: value },
    ),
  );
}

export function subscribeCollapsedProjectRailMode(onStoreChange: () => void) {
  if (typeof window === "undefined") return () => {};
  window.addEventListener(
    COLLAPSED_PROJECT_RAIL_MODE_CHANGE_EVENT,
    onStoreChange,
  );
  return () =>
    window.removeEventListener(
      COLLAPSED_PROJECT_RAIL_MODE_CHANGE_EVENT,
      onStoreChange,
    );
}

export type ModelControls = "menu" | "beside";

export const MODEL_CONTROLS_DEFAULT: ModelControls = "menu";

/** Fired on `window` when the composer model controls setting flips. */
export const MODEL_CONTROLS_CHANGE_EVENT = "monocode:model-controls-change";

export function loadModelControls(): ModelControls {
  try {
    const raw = localStorage.getItem(MODEL_CONTROLS_KEY);
    if (raw === "menu" || raw === "beside") return raw;
    if (raw == null) {
      // Migrate the previous effort-control toggle: on means beside the picker.
      const legacy = localStorage.getItem(COMPOSER_EFFORT_VISIBLE_KEY);
      if (legacy === "1" || legacy === "true") return "beside";
    }
  } catch {
    // private mode / quota
  }
  return MODEL_CONTROLS_DEFAULT;
}

export function saveModelControls(value: ModelControls) {
  try {
    localStorage.setItem(MODEL_CONTROLS_KEY, value);
  } catch {
    // private mode / quota
  }
  if (typeof window === "undefined") return;
  window.dispatchEvent(
    new CustomEvent<ModelControls>(MODEL_CONTROLS_CHANGE_EVENT, {
      detail: value,
    }),
  );
}

export function subscribeModelControls(onStoreChange: () => void) {
  if (typeof window === "undefined") return () => {};
  window.addEventListener(MODEL_CONTROLS_CHANGE_EVENT, onStoreChange);
  return () =>
    window.removeEventListener(MODEL_CONTROLS_CHANGE_EVENT, onStoreChange);
}

export const COMPOSER_RUNNER_DEFAULT = true;

/** Fired on `window` when the composer mascot setting flips. */
export const COMPOSER_RUNNER_CHANGE_EVENT = "monocode:composer-runner-change";

export function loadComposerRunner(): boolean {
  return readFlag(COMPOSER_RUNNER_KEY) ?? COMPOSER_RUNNER_DEFAULT;
}

export function saveComposerRunner(value: boolean) {
  writeFlag(COMPOSER_RUNNER_KEY, value);
  if (typeof window === "undefined") return;
  window.dispatchEvent(
    new CustomEvent<boolean>(COMPOSER_RUNNER_CHANGE_EVENT, { detail: value }),
  );
}

const NOTES_ENABLED_KEY = "monocode.notesEnabled";

export const NOTES_ENABLED_DEFAULT = true;

/** Fired on `window` when the Notes UI setting flips. */
export const NOTES_ENABLED_CHANGE_EVENT = "monocode:notes-enabled-change";

export function loadNotesEnabled(): boolean {
  return readFlag(NOTES_ENABLED_KEY) ?? NOTES_ENABLED_DEFAULT;
}

export function saveNotesEnabled(value: boolean) {
  writeFlag(NOTES_ENABLED_KEY, value);
  if (typeof window === "undefined") return;
  window.dispatchEvent(
    new CustomEvent<boolean>(NOTES_ENABLED_CHANGE_EVENT, { detail: value }),
  );
}

export function subscribeNotesEnabled(onStoreChange: () => void) {
  if (typeof window === "undefined") return () => {};
  window.addEventListener(NOTES_ENABLED_CHANGE_EVENT, onStoreChange);
  return () =>
    window.removeEventListener(NOTES_ENABLED_CHANGE_EVENT, onStoreChange);
}

const QUICK_COMPOSER_ENABLED_KEY = "monocode.quickComposerEnabled";
const QUICK_COMPOSER_SHORTCUT_KEY = "monocode.quickComposerShortcut";

export const QUICK_COMPOSER_ENABLED_DEFAULT = true;

export function loadQuickComposerEnabled(): boolean {
  return readFlag(QUICK_COMPOSER_ENABLED_KEY) ?? QUICK_COMPOSER_ENABLED_DEFAULT;
}

export function saveQuickComposerEnabled(value: boolean) {
  writeFlag(QUICK_COMPOSER_ENABLED_KEY, value);
}

export function loadQuickComposerShortcut(): string {
  try {
    const value = localStorage.getItem(QUICK_COMPOSER_SHORTCUT_KEY);
    return value && isGlobalShortcut(value)
      ? value
      : QUICK_COMPOSER_DEFAULT_SHORTCUT;
  } catch {
    return QUICK_COMPOSER_DEFAULT_SHORTCUT;
  }
}

export function saveQuickComposerShortcut(value: string) {
  if (!isGlobalShortcut(value)) return;
  try {
    localStorage.setItem(QUICK_COMPOSER_SHORTCUT_KEY, value);
  } catch {
    // private mode / quota
  }
}

const LIVE_AGENTS_ENABLED_KEY = "monocode.liveAgentsEnabled";

export const LIVE_AGENTS_ENABLED_DEFAULT = true;

/** Fired on `window` when the working-agents rail card setting flips. */
export const LIVE_AGENTS_ENABLED_CHANGE_EVENT =
  "monocode:live-agents-enabled-change";

export function loadLiveAgentsEnabled(): boolean {
  return readFlag(LIVE_AGENTS_ENABLED_KEY) ?? LIVE_AGENTS_ENABLED_DEFAULT;
}

export function saveLiveAgentsEnabled(value: boolean) {
  writeFlag(LIVE_AGENTS_ENABLED_KEY, value);
  if (typeof window === "undefined") return;
  window.dispatchEvent(
    new CustomEvent<boolean>(LIVE_AGENTS_ENABLED_CHANGE_EVENT, {
      detail: value,
    }),
  );
}

export function subscribeLiveAgentsEnabled(onStoreChange: () => void) {
  if (typeof window === "undefined") return () => {};
  window.addEventListener(LIVE_AGENTS_ENABLED_CHANGE_EVENT, onStoreChange);
  return () =>
    window.removeEventListener(LIVE_AGENTS_ENABLED_CHANGE_EVENT, onStoreChange);
}

const CLOSE_TO_TRAY_KEY = "monocode.closeToTray";

export const CLOSE_TO_TRAY_DEFAULT = true;

export function loadCloseToTray(): boolean {
  // Close to tray is Windows-only: nowhere else installs a tray icon.
  if (!IS_WIN) return false;
  return readFlag(CLOSE_TO_TRAY_KEY) ?? CLOSE_TO_TRAY_DEFAULT;
}

export function saveCloseToTray(value: boolean) {
  writeFlag(CLOSE_TO_TRAY_KEY, value);
}

const GRID_ARCADE_ENABLED_KEY = "monocode.gridArcadeEnabled";

export const GRID_ARCADE_ENABLED_DEFAULT = true;

/** Fired on `window` when the empty-session games setting flips. */
export const GRID_ARCADE_ENABLED_CHANGE_EVENT =
  "monocode:grid-arcade-enabled-change";

export function loadGridArcadeEnabled(): boolean {
  return readFlag(GRID_ARCADE_ENABLED_KEY) ?? GRID_ARCADE_ENABLED_DEFAULT;
}

export function saveGridArcadeEnabled(value: boolean) {
  writeFlag(GRID_ARCADE_ENABLED_KEY, value);
  if (typeof window === "undefined") return;
  window.dispatchEvent(
    new CustomEvent<boolean>(GRID_ARCADE_ENABLED_CHANGE_EVENT, {
      detail: value,
    }),
  );
}

export function subscribeGridArcadeEnabled(onStoreChange: () => void) {
  if (typeof window === "undefined") return () => {};
  window.addEventListener(GRID_ARCADE_ENABLED_CHANGE_EVENT, onStoreChange);
  return () =>
    window.removeEventListener(GRID_ARCADE_ENABLED_CHANGE_EVENT, onStoreChange);
}

const DIFF_VIEWER_KEY = "monocode.diffViewer";

export type DiffViewer = "editor" | "unified";

export const DIFF_VIEWER_DEFAULT: DiffViewer = "editor";

/** Fired on `window` when the working-tree diff layout flips. */
export const DIFF_VIEWER_CHANGE_EVENT = "monocode:diff-viewer-change";

function isDiffViewer(value: unknown): value is DiffViewer {
  return value === "editor" || value === "unified";
}

export function loadDiffViewer(): DiffViewer {
  try {
    const raw = localStorage.getItem(DIFF_VIEWER_KEY);
    return isDiffViewer(raw) ? raw : DIFF_VIEWER_DEFAULT;
  } catch {
    return DIFF_VIEWER_DEFAULT;
  }
}

export function saveDiffViewer(value: DiffViewer) {
  const next = isDiffViewer(value) ? value : DIFF_VIEWER_DEFAULT;
  try {
    localStorage.setItem(DIFF_VIEWER_KEY, next);
  } catch {
    // private mode / quota
  }
  if (typeof window === "undefined") return;
  window.dispatchEvent(
    new CustomEvent<DiffViewer>(DIFF_VIEWER_CHANGE_EVENT, { detail: next }),
  );
}

export function subscribeDiffViewer(onStoreChange: () => void) {
  if (typeof window === "undefined") return () => {};
  window.addEventListener(DIFF_VIEWER_CHANGE_EVENT, onStoreChange);
  return () =>
    window.removeEventListener(DIFF_VIEWER_CHANGE_EVENT, onStoreChange);
}

const FORMAT_ON_SAVE_KEY = "monocode.formatOnSave";

export const FORMAT_ON_SAVE_DEFAULT = true;

export function loadFormatOnSave(): boolean {
  return readFlag(FORMAT_ON_SAVE_KEY) ?? FORMAT_ON_SAVE_DEFAULT;
}

export function saveFormatOnSave(value: boolean) {
  writeFlag(FORMAT_ON_SAVE_KEY, value);
}

const CLAUDE_HOOKS_KEY = "monocode.claudeHooks";

export const CLAUDE_HOOKS_DEFAULT = true;

export function loadClaudeHooks(): boolean {
  return readFlag(CLAUDE_HOOKS_KEY) ?? CLAUDE_HOOKS_DEFAULT;
}

export function saveClaudeHooks(value: boolean) {
  writeFlag(CLAUDE_HOOKS_KEY, value);
}

const CTRL = IS_MAC ? "⌃" : "Ctrl+";

export type KeybindingRow = {
  command: string;
  keys: string;
  when: string;
};

/**
 * Mirrors the bindings we actually handle: the native menu accelerators in
 * `src-tauri/src/menu.rs`, `tabCommand`, the window key handler in App, and
 * focused surface handlers such as the draft composer workspace toggle.
 */
export const KEYBINDINGS: KeybindingRow[] = [
  { command: "App: Settings", keys: `${MOD},`, when: "Always" },
  { command: "App: Search", keys: `${MOD}K`, when: "Always" },
  { command: "App: Go to File", keys: `${MOD}P`, when: "Always" },
  { command: "App: Command Palette", keys: `${MOD}${SHIFT}P`, when: "Always" },
  { command: "App: Find in Files", keys: `${MOD}${SHIFT}F`, when: "Always" },
  { command: "App: Open Project", keys: `${MOD}O`, when: "Always" },
  { command: "App: New Window", keys: `${MOD}${SHIFT}N`, when: "Always" },
  ...(IS_MAC
    ? [
        {
          command: "App: Quick Composer",
          keys: `${MOD}${SHIFT}Space`,
          when: "Anywhere",
        },
      ]
    : []),
  { command: "App: Toggle Sidebar", keys: `${MOD}B`, when: "Always" },
  {
    command: "App: Toggle Session Sidebar",
    keys: `${MOD}${SHIFT}B`,
    when: "Always",
  },
  { command: "App: Switch Model", keys: `${MOD}.`, when: "Always" },
  {
    command: "Composer: Toggle Workspace",
    keys: `${MOD}${SHIFT}G`,
    when: "Draft session composer",
  },
  { command: "View: Reload", keys: `${MOD}${SHIFT}R`, when: "Always" },
  { command: "View: Zoom In", keys: `${MOD}+`, when: "Always" },
  { command: "View: Zoom Out", keys: `${MOD}-`, when: "Always" },
  { command: "View: Reset Zoom", keys: `${MOD}0`, when: "Always" },
  { command: "Tab: New", keys: `${MOD}T`, when: "Always" },
  { command: "Tab: Close Others", keys: `${MOD}${ALT}T`, when: "Always" },
  { command: "Tab: Close All", keys: `${MOD}${SHIFT}W`, when: "Always" },
  { command: "Tab: Next", keys: `${MOD}${SHIFT}]`, when: "Always" },
  { command: "Tab: Previous", keys: `${MOD}${SHIFT}[`, when: "Always" },
  { command: "Tab: Cycle Next", keys: `${CTRL}Tab`, when: "Always" },
  {
    command: "Tab: Cycle Previous",
    keys: `${CTRL}${SHIFT}Tab`,
    when: "Always",
  },
  { command: "Tab: Back", keys: `${MOD}[`, when: "Always" },
  { command: "Tab: Forward", keys: `${MOD}]`, when: "Always" },
  { command: "Tab: Activate 1–8", keys: `${MOD}1 … ${MOD}8`, when: "Always" },
  { command: "Tab: Activate Last", keys: `${MOD}9`, when: "Always" },
  {
    command: "Session: Archive",
    keys: `${MOD}${SHIFT}A`,
    when: "sessionFocus && !overlay",
  },
  {
    command: "Session: Previous",
    keys: `${MOD}${SHIFT}↑`,
    when: "!overlay && (!textFocus || emptyComposer)",
  },
  {
    command: "Session: Next",
    keys: `${MOD}${SHIFT}↓`,
    when: "!overlay && (!textFocus || emptyComposer)",
  },
  {
    command: "Session: Previous in Current Tab",
    keys: `${MOD}↑`,
    when: "!overlay && (!textFocus || emptyComposer)",
  },
  {
    command: "Session: Next in Current Tab",
    keys: `${MOD}↓`,
    when: "!overlay && (!textFocus || emptyComposer)",
  },
  {
    command: "Project: Previous",
    keys: `${MOD}${SHIFT}←`,
    when: "!overlay && (!textFocus || emptyComposer)",
  },
  {
    command: "Project: Next",
    keys: `${MOD}${SHIFT}→`,
    when: "!overlay && (!textFocus || emptyComposer)",
  },
  { command: "Pane: Close", keys: `${MOD}W`, when: "Always" },
  { command: "Pane: Split Right", keys: `${MOD}D`, when: "!editorFocus" },
  {
    command: "Pane: Split Down",
    keys: `${MOD}${SHIFT}D`,
    when: "!editorFocus",
  },
  { command: "Pane: Focus Left", keys: `${MOD}${ALT}←`, when: "Always" },
  { command: "Pane: Focus Right", keys: `${MOD}${ALT}→`, when: "Always" },
  { command: "Pane: Focus Up", keys: `${MOD}${ALT}↑`, when: "Always" },
  { command: "Pane: Focus Down", keys: `${MOD}${ALT}↓`, when: "Always" },
  { command: "Terminal: New", keys: `${MOD}\``, when: "Always" },
  { command: "Terminal: New Tab", keys: `${MOD}${SHIFT}\``, when: "Always" },
  { command: "Terminal: Toggle Dock", keys: `${MOD}J`, when: "Always" },
  { command: "Editor: Find", keys: `${MOD}F`, when: "editorFocus" },
  { command: "Editor: Replace", keys: `${MOD}${ALT}F`, when: "editorFocus" },
];

const KEYBINDING_OVERRIDES_KEY = "monocode.keybindingOverrides";
const KEYBINDINGS_CHANGE_EVENT = "monocode:keybindings-change";

export type KeybindingOverride = {
  disabled?: boolean;
  shortcut?: string;
};

export type KeybindingOverrides = Record<string, KeybindingOverride>;

const VALID_COMMANDS = new Set(KEYBINDINGS.map((row) => row.command));

const KEY_CODES: Record<string, string> = {
  " ": "Space",
  Enter: "Enter",
  Space: "Space",
  Tab: "Tab",
  "`": "Backquote",
  "[": "BracketLeft",
  "]": "BracketRight",
  ",": "Comma",
  ".": "Period",
  "+": "Equal",
  "-": "Minus",
  "\\": "Backslash",
  "↑": "ArrowUp",
  "↓": "ArrowDown",
  "←": "ArrowLeft",
  "→": "ArrowRight",
};

const DISPLAY_MODIFIERS: [string, string][] = IS_MAC
  ? [
      ["⌘", "Command"],
      ["⌃", "Control"],
      ["⌥", "Option"],
      ["⇧", "Shift"],
    ]
  : [
      ["Ctrl+", "Control"],
      ["Alt+", "Option"],
      ["Shift+", "Shift"],
    ];

/**
 * The stored form of a row's documented default, so a recorded chord can be
 * checked against every other command and not only against other overrides.
 * Returns null for grouped rows that document a range rather than one chord.
 */
function defaultShortcutFor(command: string): string | null {
  const row = KEYBINDINGS.find((entry) => entry.command === command);
  if (!row || row.keys.includes("…")) return null;
  let rest = row.keys;
  const modifiers: string[] = [];
  for (const [display, modifier] of DISPLAY_MODIFIERS) {
    if (rest.startsWith(display)) {
      modifiers.push(modifier);
      rest = rest.slice(display.length);
    }
  }
  const code = /^[A-Za-z]$/.test(rest)
    ? `Key${rest.toUpperCase()}`
    : /^[0-9]$/.test(rest)
      ? `Digit${rest}`
      : (KEY_CODES[rest] ??
        (/^F(?:[1-9]|1[0-9]|2[0-4])$/.test(rest) ? rest : null));
  if (!code) return null;
  return canonicalShortcut(
    modifiers.length ? [...modifiers, code].join("+") : code,
  );
}

function validateShortcut(command: string, shortcut: string): string {
  const canonical = canonicalShortcut(shortcut);
  if (!canonical) throw new Error("That combination is not a valid shortcut");
  if (command === "Tab: Activate 1–8" && !/Digit[1-8]$/.test(canonical)) {
    throw new Error("Tab: Activate 1–8 needs a number key from 1 to 8");
  }
  return canonical;
}

export function loadKeybindingOverrides(): KeybindingOverrides {
  try {
    const value = JSON.parse(
      localStorage.getItem(KEYBINDING_OVERRIDES_KEY) ?? "{}",
    );
    if (!value || typeof value !== "object" || Array.isArray(value)) return {};
    const next: KeybindingOverrides = {};
    for (const [command, entry] of Object.entries(
      value as Record<string, unknown>,
    )) {
      if (!VALID_COMMANDS.has(command) || !entry || typeof entry !== "object")
        continue;
      const override = entry as { disabled?: unknown; shortcut?: unknown };
      const disabled = override.disabled === true;
      const shortcut =
        typeof override.shortcut === "string"
          ? (canonicalShortcut(override.shortcut) ?? undefined)
          : undefined;
      if (shortcut) next[command] = { shortcut };
      else if (disabled) next[command] = { disabled: true };
    }
    return next;
  } catch {
    return {};
  }
}

export function saveKeybindingOverride(
  command: string,
  override: KeybindingOverride,
): KeybindingOverrides {
  const next = loadKeybindingOverrides();
  if (override.disabled) next[command] = { disabled: true };
  else if (override.shortcut) {
    const shortcut = validateShortcut(command, override.shortcut);
    const conflict = [
      ...Object.entries(next).map(
        ([other, value]) => [other, value.shortcut] as const,
      ),
      ...KEYBINDINGS.map(
        (row) => [row.command, defaultShortcutFor(row.command)] as const,
      ),
    ].find(([other, existing]) => other !== command && existing === shortcut);
    if (conflict) throw new Error(`Already used by ${conflict[0]}`);
    next[command] = { shortcut };
  } else delete next[command];
  try {
    if (Object.keys(next).length) {
      localStorage.setItem(KEYBINDING_OVERRIDES_KEY, JSON.stringify(next));
    } else {
      localStorage.removeItem(KEYBINDING_OVERRIDES_KEY);
    }
  } catch {}
  if (typeof window !== "undefined") {
    window.dispatchEvent(new CustomEvent(KEYBINDINGS_CHANGE_EVENT));
  }
  return next;
}

export type ShortcutEvent = Pick<KeyboardEvent, "code"> &
  Pick<KeyboardEvent, "metaKey" | "ctrlKey" | "altKey" | "shiftKey">;

export function shortcutMatches(
  shortcut: string,
  event: ShortcutEvent,
): boolean {
  // Copy the fields: real keyboard events expose modifiers as prototype
  // accessors, so spreading the event would silently drop all of them.
  return (
    shortcutFromKeyEvent({
      code: event.code,
      metaKey: event.metaKey,
      ctrlKey: event.ctrlKey,
      altKey: event.altKey,
      shiftKey: event.shiftKey,
    }) === shortcut
  );
}

export function matchCustomKeybinding(event: ShortcutEvent): string | null {
  for (const [command, override] of Object.entries(loadKeybindingOverrides())) {
    if (override.shortcut && shortcutMatches(override.shortcut, event)) {
      return command;
    }
  }
  return null;
}

export function keybindingPressed(
  command: string,
  event: ShortcutEvent,
  defaultMatch: boolean,
): boolean {
  const override = loadKeybindingOverrides()[command];
  if (override?.disabled) return false;
  if (override?.shortcut) return shortcutMatches(override.shortcut, event);
  return defaultMatch;
}

export function keybindingShortcutLabel(
  command: string,
  fallback: string,
): string | null {
  const override = loadKeybindingOverrides()[command];
  if (override?.disabled) return null;
  return override?.shortcut
    ? quickComposerShortcutLabel(override.shortcut)
    : fallback;
}

export function subscribeKeybindings(onStoreChange: () => void) {
  if (typeof window === "undefined") return () => {};
  const onStorage = (event: StorageEvent) => {
    if (event.key === KEYBINDING_OVERRIDES_KEY) onStoreChange();
  };
  window.addEventListener(KEYBINDINGS_CHANGE_EVENT, onStoreChange);
  window.addEventListener("storage", onStorage);
  return () => {
    window.removeEventListener(KEYBINDINGS_CHANGE_EVENT, onStoreChange);
    window.removeEventListener("storage", onStorage);
  };
}

export function currentKeybindings(): KeybindingRow[] {
  const overrides = loadKeybindingOverrides();
  return KEYBINDINGS.map((row) => {
    if (row.command === "App: Quick Composer") {
      return {
        ...row,
        keys: loadQuickComposerEnabled()
          ? quickComposerShortcutLabel(loadQuickComposerShortcut())
          : "Disabled",
      };
    }
    const override = overrides[row.command];
    return {
      ...row,
      keys: override?.disabled
        ? "Disabled"
        : override?.shortcut
          ? quickComposerShortcutLabel(override.shortcut)
          : row.keys,
    };
  });
}

export function filterKeybindings(
  rows: KeybindingRow[],
  query: string,
): KeybindingRow[] {
  const needle = query.trim().toLowerCase();
  if (!needle) return rows;
  return rows.filter(
    (row) =>
      row.command.toLowerCase().includes(needle) ||
      row.keys.toLowerCase().includes(needle) ||
      row.when.toLowerCase().includes(needle),
  );
}
