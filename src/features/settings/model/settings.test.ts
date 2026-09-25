import { afterEach, beforeEach, describe, expect, it } from "vitest";
import {
  COMPOSER_RUNNER_DEFAULT,
  COLLAPSED_PROJECT_RAIL_MODE_DEFAULT,
  searchSettings,
  SETTINGS_INDEX,
  settingsSectionsByGroup,
  MODEL_CONTROLS_DEFAULT,
  DIFF_VIEWER_DEFAULT,
  FORMAT_ON_SAVE_DEFAULT,
  FILE_TAB_MODE_DEFAULT,
  FOLLOW_UP_BEHAVIOR_DEFAULT,
  GRID_ARCADE_ENABLED_DEFAULT,
  KEYBINDINGS,
  LIVE_AGENTS_ENABLED_DEFAULT,
  TAB_ANIMATIONS_ENABLED_DEFAULT,
  loadComposerRunner,
  loadCollapsedProjectRailMode,
  loadModelControls,
  loadDiffViewer,
  loadKeybindingOverrides,
  loadFormatOnSave,
  loadFileTabMode,
  loadFollowUpBehavior,
  loadGridArcadeEnabled,
  loadLiveAgentsEnabled,
  loadNotesEnabled,
  keybindingPressed,
  matchCustomKeybinding,
  loadQuickComposerShortcut,
  loadTabAnimationsEnabled,
  NOTES_ENABLED_DEFAULT,
  saveComposerRunner,
  saveCollapsedProjectRailMode,
  saveModelControls,
  saveDiffViewer,
  saveFormatOnSave,
  saveFileTabMode,
  saveFollowUpBehavior,
  saveGridArcadeEnabled,
  saveLiveAgentsEnabled,
  saveNotesEnabled,
  saveKeybindingOverride,
  saveQuickComposerShortcut,
  saveTabAnimationsEnabled,
} from "./settings";
import { MOD, SHIFT } from "../../../platform/tauri/platform";

const KEY = "monocode.composerRunner";
const MODEL_CONTROLS_KEY = "monocode.modelControls";
const LEGACY_EFFORT_VISIBLE_KEY = "monocode.composerEffortVisible";
const NOTES_KEY = "monocode.notesEnabled";
const KEYBINDING_OVERRIDES_KEY = "monocode.keybindingOverrides";
const QUICK_COMPOSER_SHORTCUT_KEY = "monocode.quickComposerShortcut";
const LIVE_AGENTS_KEY = "monocode.liveAgentsEnabled";
const GRID_ARCADE_KEY = "monocode.gridArcadeEnabled";
const DIFF_VIEWER_KEY = "monocode.diffViewer";
const FORMAT_ON_SAVE_KEY = "monocode.formatOnSave";
const FILE_TAB_MODE_KEY = "monocode.fileTabMode";
const FOLLOW_UP_BEHAVIOR_KEY = "monocode.followUpBehavior";
const TAB_ANIMATIONS_KEY = "monocode.tabAnimationsEnabled";
const COLLAPSED_PROJECT_RAIL_MODE_KEY = "monocode.collapsedProjectRailMode";

describe("follow-up behavior setting", () => {
  beforeEach(mockLocalStorage);
  afterEach(() => {
    localStorage.removeItem(FOLLOW_UP_BEHAVIOR_KEY);
  });

  it("defaults to steer", () => {
    expect(FOLLOW_UP_BEHAVIOR_DEFAULT).toBe("steer");
    expect(loadFollowUpBehavior()).toBe("steer");
  });

  it("persists queue behavior", () => {
    saveFollowUpBehavior("queue");
    expect(loadFollowUpBehavior()).toBe("queue");
  });

  it("ignores unknown stored values", () => {
    localStorage.setItem(FOLLOW_UP_BEHAVIOR_KEY, "interrupt");
    expect(loadFollowUpBehavior()).toBe("steer");
  });
});

function mockLocalStorage() {
  const data = new Map<string, string>();
  const storage = {
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => {
      data.set(key, value);
    },
    removeItem: (key: string) => {
      data.delete(key);
    },
    clear: () => {
      data.clear();
    },
    key: (index: number) => [...data.keys()][index] ?? null,
    get length() {
      return data.size;
    },
  };
  Object.defineProperty(globalThis, "localStorage", {
    value: storage,
    configurable: true,
  });
}

describe("composer runner setting", () => {
  beforeEach(mockLocalStorage);
  afterEach(() => {
    localStorage.removeItem(KEY);
  });

  it("defaults to on", () => {
    expect(COMPOSER_RUNNER_DEFAULT).toBe(true);
    expect(loadComposerRunner()).toBe(true);
  });

  it("persists an off switch", () => {
    saveComposerRunner(false);
    expect(localStorage.getItem(KEY)).toBe("0");
    expect(loadComposerRunner()).toBe(false);
    saveComposerRunner(true);
    expect(loadComposerRunner()).toBe(true);
  });
});

describe("model controls setting", () => {
  beforeEach(mockLocalStorage);
  afterEach(() => {
    localStorage.removeItem(MODEL_CONTROLS_KEY);
    localStorage.removeItem(LEGACY_EFFORT_VISIBLE_KEY);
  });

  it("keeps options in the model menu by default", () => {
    expect(MODEL_CONTROLS_DEFAULT).toBe("menu");
    expect(loadModelControls()).toBe("menu");
  });

  it("persists the beside-picker preference", () => {
    saveModelControls("beside");
    expect(localStorage.getItem(MODEL_CONTROLS_KEY)).toBe("beside");
    expect(loadModelControls()).toBe("beside");
    saveModelControls("menu");
    expect(loadModelControls()).toBe("menu");
  });

  it("ignores unknown stored values", () => {
    localStorage.setItem(MODEL_CONTROLS_KEY, "everywhere");
    expect(loadModelControls()).toBe("menu");
  });

  it("migrates the previous effort-control toggle", () => {
    localStorage.setItem(LEGACY_EFFORT_VISIBLE_KEY, "1");
    expect(loadModelControls()).toBe("beside");
    localStorage.setItem(LEGACY_EFFORT_VISIBLE_KEY, "0");
    localStorage.removeItem(MODEL_CONTROLS_KEY);
    expect(loadModelControls()).toBe("menu");
  });
});

describe("notes enabled setting", () => {
  beforeEach(mockLocalStorage);
  afterEach(() => {
    localStorage.removeItem(NOTES_KEY);
  });

  it("defaults to on", () => {
    expect(NOTES_ENABLED_DEFAULT).toBe(true);
    expect(loadNotesEnabled()).toBe(true);
  });

  it("persists an off switch", () => {
    saveNotesEnabled(false);
    expect(localStorage.getItem(NOTES_KEY)).toBe("0");
    expect(loadNotesEnabled()).toBe(false);
    saveNotesEnabled(true);
    expect(loadNotesEnabled()).toBe(true);
  });
});

describe("keybinding overrides", () => {
  beforeEach(mockLocalStorage);

  const key = (
    code: string,
    modifiers: Partial<{
      metaKey: boolean;
      ctrlKey: boolean;
      altKey: boolean;
      shiftKey: boolean;
    }> = {},
  ) => ({
    code,
    metaKey: false,
    ctrlKey: false,
    altKey: false,
    shiftKey: false,
    ...modifiers,
  });

  it("persists a custom shortcut and matches only that combination", () => {
    saveKeybindingOverride("App: Search", { shortcut: "Command+Shift+KeyM" });
    expect(localStorage.getItem(KEYBINDING_OVERRIDES_KEY)).toBe(
      '{"App: Search":{"shortcut":"Command+Shift+KeyM"}}',
    );
    expect(
      keybindingPressed(
        "App: Search",
        key("KeyM", { metaKey: true, shiftKey: true }),
        true,
      ),
    ).toBe(true);
    expect(
      keybindingPressed("App: Search", key("KeyK", { metaKey: true }), true),
    ).toBe(false);
    expect(
      matchCustomKeybinding(key("KeyM", { metaKey: true, shiftKey: true })),
    ).toBe("App: Search");

    saveKeybindingOverride("App: Search", {});
    expect(localStorage.getItem(KEYBINDING_OVERRIDES_KEY)).toBeNull();
  });

  it("disables a shortcut and restores the default", () => {
    saveKeybindingOverride("App: Search", { disabled: true });
    expect(loadKeybindingOverrides()).toEqual({
      "App: Search": { disabled: true },
    });
    expect(
      keybindingPressed("App: Search", key("KeyK", { metaKey: true }), true),
    ).toBe(false);
  });

  it("rejects a shortcut already used by another command", () => {
    saveKeybindingOverride("App: Search", { shortcut: "Command+KeyY" });
    expect(() =>
      saveKeybindingOverride("App: Go to File", { shortcut: "Command+KeyY" }),
    ).toThrow("Already used by App: Search");
  });

  it("ignores malformed, unknown, and invalid stored overrides", () => {
    localStorage.setItem(
      KEYBINDING_OVERRIDES_KEY,
      JSON.stringify({
        "Unknown: Command": { disabled: true },
        "App: Search": { shortcut: "KeyK" },
        "Tab: New": { shortcut: "Command+KeyT" },
      }),
    );
    expect(loadKeybindingOverrides()).toEqual({
      "Tab: New": { shortcut: "Command+KeyT" },
    });
    localStorage.setItem(KEYBINDING_OVERRIDES_KEY, "not-json");
    expect(loadKeybindingOverrides()).toEqual({});
  });
});

describe("quick composer shortcut setting", () => {
  beforeEach(mockLocalStorage);

  it("defaults to the existing shortcut and persists a custom binding", () => {
    expect(loadQuickComposerShortcut()).toBe("Command+Shift+Space");
    saveQuickComposerShortcut("Command+Option+KeyK");
    expect(localStorage.getItem(QUICK_COMPOSER_SHORTCUT_KEY)).toBe(
      "Command+Option+KeyK",
    );
    expect(loadQuickComposerShortcut()).toBe("Command+Option+KeyK");
  });

  it("ignores malformed stored bindings", () => {
    localStorage.setItem(QUICK_COMPOSER_SHORTCUT_KEY, "Shift+Space");
    expect(loadQuickComposerShortcut()).toBe("Command+Shift+Space");
  });
});

describe("live agents enabled setting", () => {
  beforeEach(mockLocalStorage);
  afterEach(() => {
    localStorage.removeItem(LIVE_AGENTS_KEY);
  });

  it("defaults to on", () => {
    expect(LIVE_AGENTS_ENABLED_DEFAULT).toBe(true);
    expect(loadLiveAgentsEnabled()).toBe(true);
  });

  it("persists an off switch", () => {
    saveLiveAgentsEnabled(false);
    expect(localStorage.getItem(LIVE_AGENTS_KEY)).toBe("0");
    expect(loadLiveAgentsEnabled()).toBe(false);
    saveLiveAgentsEnabled(true);
    expect(loadLiveAgentsEnabled()).toBe(true);
  });
});

describe("grid arcade enabled setting", () => {
  beforeEach(mockLocalStorage);
  afterEach(() => {
    localStorage.removeItem(GRID_ARCADE_KEY);
  });

  it("defaults to on", () => {
    expect(GRID_ARCADE_ENABLED_DEFAULT).toBe(true);
    expect(loadGridArcadeEnabled()).toBe(true);
  });

  it("persists an off switch", () => {
    saveGridArcadeEnabled(false);
    expect(localStorage.getItem(GRID_ARCADE_KEY)).toBe("0");
    expect(loadGridArcadeEnabled()).toBe(false);
    saveGridArcadeEnabled(true);
    expect(loadGridArcadeEnabled()).toBe(true);
  });
});

describe("workspace navigation keybindings", () => {
  it("keeps separate shortcuts for the project rail and session sidebar", () => {
    expect(
      KEYBINDINGS.filter((row) =>
        ["App: Toggle Sidebar", "App: Toggle Session Sidebar"].includes(
          row.command,
        ),
      ),
    ).toEqual([
      { command: "App: Toggle Sidebar", keys: `${MOD}B`, when: "Always" },
      {
        command: "App: Toggle Session Sidebar",
        keys: `${MOD}${SHIFT}B`,
        when: "Always",
      },
    ]);
  });
  it("documents the command palette and reload shortcuts", () => {
    expect(
      KEYBINDINGS.filter((row) =>
        ["App: Command Palette", "View: Reload"].includes(row.command),
      ),
    ).toEqual([
      {
        command: "App: Command Palette",
        keys: `${MOD}${SHIFT}P`,
        when: "Always",
      },
      {
        command: "View: Reload",
        keys: `${MOD}${SHIFT}R`,
        when: "Always",
      },
    ]);
  });
  it("documents the draft workspace toggle", () => {
    expect(
      KEYBINDINGS.find((row) => row.command === "Composer: Toggle Workspace"),
    ).toEqual({
      command: "Composer: Toggle Workspace",
      keys: `${MOD}${SHIFT}G`,
      when: "Draft session composer",
    });
  });
  it("documents session and project cycling in the shortcut list", () => {
    const rows = KEYBINDINGS.filter((row) =>
      /^(Session|Project): (Previous|Next)$/.test(row.command),
    );
    expect(rows.map((row) => row.command)).toEqual([
      "Session: Previous",
      "Session: Next",
      "Project: Previous",
      "Project: Next",
    ]);
    expect(
      rows.every(
        (row) => row.when === "!overlay && (!textFocus || emptyComposer)",
      ),
    ).toBe(true);
  });
  it("documents same-tab session switching", () => {
    expect(
      KEYBINDINGS.filter((row) => row.command.includes("in Current Tab")),
    ).toEqual([
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
    ]);
  });
});

describe("format on save setting", () => {
  beforeEach(mockLocalStorage);
  afterEach(() => {
    localStorage.removeItem(FORMAT_ON_SAVE_KEY);
  });

  it("defaults to on", () => {
    expect(FORMAT_ON_SAVE_DEFAULT).toBe(true);
    expect(loadFormatOnSave()).toBe(true);
  });

  it("persists an off switch", () => {
    saveFormatOnSave(false);
    expect(localStorage.getItem(FORMAT_ON_SAVE_KEY)).toBe("0");
    expect(loadFormatOnSave()).toBe(false);
    saveFormatOnSave(true);
    expect(loadFormatOnSave()).toBe(true);
  });
});

describe("diff viewer setting", () => {
  beforeEach(mockLocalStorage);
  afterEach(() => {
    localStorage.removeItem(DIFF_VIEWER_KEY);
  });

  it("defaults to the editor layout", () => {
    expect(DIFF_VIEWER_DEFAULT).toBe("editor");
    expect(loadDiffViewer()).toBe("editor");
  });

  it("persists the unified layout", () => {
    saveDiffViewer("unified");
    expect(localStorage.getItem(DIFF_VIEWER_KEY)).toBe("unified");
    expect(loadDiffViewer()).toBe("unified");
    saveDiffViewer("editor");
    expect(loadDiffViewer()).toBe("editor");
  });

  it("ignores unknown stored values", () => {
    localStorage.setItem(DIFF_VIEWER_KEY, "split");
    expect(loadDiffViewer()).toBe("editor");
  });
});

describe("file tab mode setting", () => {
  beforeEach(mockLocalStorage);
  afterEach(() => {
    localStorage.removeItem(FILE_TAB_MODE_KEY);
  });

  it("opens files beside chat by default", () => {
    expect(FILE_TAB_MODE_DEFAULT).toBe("pane");
    expect(loadFileTabMode()).toBe("pane");
  });

  it("persists top-level file tabs", () => {
    saveFileTabMode("workspace");
    expect(localStorage.getItem(FILE_TAB_MODE_KEY)).toBe("workspace");
    expect(loadFileTabMode()).toBe("workspace");
  });

  it("ignores unknown stored values", () => {
    localStorage.setItem(FILE_TAB_MODE_KEY, "window");
    expect(loadFileTabMode()).toBe("pane");
  });
});

describe("tab animations setting", () => {
  beforeEach(mockLocalStorage);
  afterEach(() => {
    localStorage.removeItem(TAB_ANIMATIONS_KEY);
  });

  it("defaults to off", () => {
    expect(TAB_ANIMATIONS_ENABLED_DEFAULT).toBe(false);
    expect(loadTabAnimationsEnabled()).toBe(false);
  });

  it("persists an off switch", () => {
    saveTabAnimationsEnabled(false);
    expect(localStorage.getItem(TAB_ANIMATIONS_KEY)).toBe("0");
    expect(loadTabAnimationsEnabled()).toBe(false);
    saveTabAnimationsEnabled(true);
    expect(loadTabAnimationsEnabled()).toBe(true);
  });
});

describe("collapsed project rail setting", () => {
  beforeEach(mockLocalStorage);
  afterEach(() => {
    localStorage.removeItem(COLLAPSED_PROJECT_RAIL_MODE_KEY);
  });

  it("defaults to the icon rail", () => {
    expect(COLLAPSED_PROJECT_RAIL_MODE_DEFAULT).toBe("compact");
    expect(loadCollapsedProjectRailMode()).toBe("compact");
  });

  it("persists the hidden mode and ignores unknown values", () => {
    saveCollapsedProjectRailMode("hidden");
    expect(localStorage.getItem(COLLAPSED_PROJECT_RAIL_MODE_KEY)).toBe(
      "hidden",
    );
    expect(loadCollapsedProjectRailMode()).toBe("hidden");

    localStorage.setItem(COLLAPSED_PROJECT_RAIL_MODE_KEY, "floating");
    expect(loadCollapsedProjectRailMode()).toBe("compact");
  });
});

describe("settings navigation", () => {
  it("lists every section under exactly one rail group", () => {
    const groups = settingsSectionsByGroup();
    expect(groups.map((group) => group.label)).toEqual([
      "App",
      "Agents",
      "Workspace",
    ]);
    expect(groups.flatMap((group) => group.sections.map((s) => s.id))).toEqual([
      "general",
      "appearance",
      "keybindings",
      "chat",
      "providers",
      "skills",
      "inbox",
      "archive",
      "worktrees",
    ]);
  });

  it("points every indexed setting at a real section", () => {
    const sections = new Set(
      settingsSectionsByGroup().flatMap((group) =>
        group.sections.map((section) => section.id),
      ),
    );
    for (const entry of SETTINGS_INDEX) {
      expect(sections.has(entry.section), entry.id).toBe(true);
    }
  });
});

describe("settings search", () => {
  it("returns nothing for an empty query", () => {
    expect(searchSettings("   ")).toEqual([]);
  });

  it("ranks label matches over keyword matches, and pages last", () => {
    expect(searchSettings("glass").map((result) => result.label)).toEqual([
      "Main pane glass",
      "Blur radius",
      "Sidebar opacity",
      "Appearance",
    ]);
  });

  it("finds a setting by a word that is not in its label", () => {
    expect(searchSettings("steer")[0]).toMatchObject({
      section: "chat",
      sectionLabel: "Chat",
      settingId: "follow-up",
      label: "Follow-up behavior",
    });

    expect(searchSettings("prettier")[0]).toMatchObject({
      section: "chat",
      settingId: "format-on-save",
      label: "Format on save",
    });
  });

  it("returns a whole page with no setting id", () => {
    expect(searchSettings("skills")).toEqual([
      {
        section: "skills",
        sectionLabel: "Skills",
        settingId: null,
        label: "Skills",
      },
    ]);
  });

  it("caps the result list", () => {
    expect(searchSettings("e", 4)).toHaveLength(4);
  });
});
