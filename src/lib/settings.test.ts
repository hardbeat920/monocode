import { afterEach, beforeEach, describe, expect, it } from "vitest";
import {
  COMPOSER_RUNNER_DEFAULT,
  searchSettings,
  SETTINGS_INDEX,
  settingsSectionsByGroup,
  COMPOSER_EFFORT_VISIBLE_DEFAULT,
  DIFF_VIEWER_DEFAULT,
  FOLLOW_UP_BEHAVIOR_DEFAULT,
  GRID_ARCADE_ENABLED_DEFAULT,
  KEYBINDINGS,
  LIVE_AGENTS_ENABLED_DEFAULT,
  loadComposerRunner,
  loadComposerEffortVisible,
  loadDiffViewer,
  loadFollowUpBehavior,
  loadGridArcadeEnabled,
  loadLiveAgentsEnabled,
  loadNotesEnabled,
  NOTES_ENABLED_DEFAULT,
  saveComposerRunner,
  saveComposerEffortVisible,
  saveDiffViewer,
  saveFollowUpBehavior,
  saveGridArcadeEnabled,
  saveLiveAgentsEnabled,
  saveNotesEnabled,
} from "./settings";
import { MOD, SHIFT } from "./platform";

const KEY = "monocode.composerRunner";
const COMPOSER_EFFORT_VISIBLE_KEY = "monocode.composerEffortVisible";
const NOTES_KEY = "monocode.notesEnabled";
const LIVE_AGENTS_KEY = "monocode.liveAgentsEnabled";
const GRID_ARCADE_KEY = "monocode.gridArcadeEnabled";
const DIFF_VIEWER_KEY = "monocode.diffViewer";
const FOLLOW_UP_BEHAVIOR_KEY = "monocode.followUpBehavior";

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

describe("composer effort control setting", () => {
  beforeEach(mockLocalStorage);
  afterEach(() => {
    localStorage.removeItem(COMPOSER_EFFORT_VISIBLE_KEY);
  });

  it("keeps effort in the model picker by default", () => {
    expect(COMPOSER_EFFORT_VISIBLE_DEFAULT).toBe(false);
    expect(loadComposerEffortVisible()).toBe(false);
  });

  it("persists the standalone effort control preference", () => {
    saveComposerEffortVisible(true);
    expect(localStorage.getItem(COMPOSER_EFFORT_VISIBLE_KEY)).toBe("1");
    expect(loadComposerEffortVisible()).toBe(true);
    saveComposerEffortVisible(false);
    expect(loadComposerEffortVisible()).toBe(false);
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
