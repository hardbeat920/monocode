// @vitest-environment happy-dom
import { act, createElement, type ComponentProps } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { ask } from "@tauri-apps/plugin-dialog";
import { SettingsView } from "./SettingsView";
import { rememberNotificationProjects } from "../../notifications/model/notificationProjects";
import {
  SETTINGS_INDEX,
  SETTINGS_SECTIONS,
  type SettingsSectionId,
} from "../model/settings";
import {
  providerAccounts,
  saveProviderAccount,
} from "../../providers/model/providerAccounts";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async () => undefined),
  convertFileSrc: (path: string) => path,
}));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    isMaximized: async () => false,
    onResized: async () => () => {},
  }),
}));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ ask: vi.fn(async () => true) }));
vi.mock("../../../integrations/harness/core/availability", () => ({
  isHarnessAvailable: (id: string) => id === "claude" || id === "cursor",
  hasProbedHarnessAvailability: () => true,
  getHarnessAvailabilitySnapshot: () => 0,
  subscribeHarnessAvailability: () => () => {},
  probeHarnessAvailability: async () => {},
  harnessUnavailableHint: () => "",
}));

let container: HTMLDivElement;
let root: Root;
let onSelectSection: ReturnType<typeof vi.fn>;

function mockLocalStorage() {
  const data = new Map<string, string>();
  vi.stubGlobal("localStorage", {
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => data.set(key, value),
    removeItem: (key: string) => data.delete(key),
    clear: () => data.clear(),
    key: (index: number) => [...data.keys()][index] ?? null,
    get length() {
      return data.size;
    },
  });
}

async function render(
  section: SettingsSectionId,
  options: Partial<ComponentProps<typeof SettingsView>> = {},
) {
  await act(async () =>
    root.render(
      createElement(SettingsView, {
        section,
        cwd: "/repo",
        sessions: [],
        onClose: vi.fn(),
        onSelectSection,
        onOpenSession: vi.fn(),
        onArchiveSession: vi.fn(),
        onDeleteSession: vi.fn(),
        onOpenWhatsNew: vi.fn(),
        ...options,
      }),
    ),
  );
}

function renderedSettingIds(): string[] {
  return Array.from(
    container.querySelectorAll<HTMLElement>("[data-setting-id]"),
    (node) => node.dataset.settingId!,
  );
}

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  mockLocalStorage();
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
  onSelectSection = vi.fn();
});

afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
  localStorage.clear();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  vi.useRealTimers();
});

describe("settings pages", () => {
  it("shows background effect choices above scope when artwork is available", async () => {
    localStorage.setItem(
      "monocode.chatBackgroundPath",
      "/app-data/backgrounds/chat-background.png",
    );
    await render("appearance");

    const effect = container.querySelector(
      "#new-thread-background-effect-dither",
    )!;
    const effectRow = effect.closest(".settings-row")!;
    const scopeRow = container
      .querySelector('[aria-label="Show background on"]')!
      .closest(".settings-row")!;
    expect(effect).not.toBeNull();
    expect(
      effectRow.compareDocumentPosition(scopeRow) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).not.toBe(0);

    vi.stubGlobal(
      "fetch",
      vi.fn(async () => ({ ok: false })),
    );
    await act(async () => (effect as HTMLButtonElement).click());
    expect(localStorage.getItem("monocode.newThreadBackgroundEffect")).toBe(
      "dither",
    );
    expect(effect.getAttribute("aria-checked")).toBe("true");
    expect(container.textContent).toContain(
      "Rebuilds the artwork with a dithered color palette.",
    );
  });

  it("previews and restores Haze with the existing empty-chat visibility", async () => {
    localStorage.setItem("monocode.chatBackgroundPath", "/background.png");
    localStorage.setItem("monocode.chatBackgroundEmptyOpacity", "0.4");
    await render("appearance");

    const option = container.querySelector<HTMLButtonElement>(
      "#new-thread-background-effect-gradient-blur",
    )!;
    expect(option.textContent).toBe("Haze");
    await act(async () => option.click());

    const preview = container.querySelector<HTMLElement>(
      ".gradient-blur-background",
    )!;
    expect(option.getAttribute("aria-checked")).toBe("true");
    expect(preview.style.opacity).toBe("0.4");
    expect(preview.querySelectorAll("span")).toHaveLength(2);
    expect(localStorage.getItem("monocode.newThreadBackgroundEffect")).toBe(
      "gradient-blur",
    );

    await render("providers");
    await render("appearance");
    expect(
      container
        .querySelector("#new-thread-background-effect-gradient-blur")
        ?.getAttribute("aria-checked"),
    ).toBe("true");
    expect(container.querySelector(".gradient-blur-background")).not.toBeNull();
  });

  it("manages named accounts independently for each supported provider", async () => {
    saveProviderAccount({
      id: "account-work",
      provider: "codex",
      label: "Wrk",
    });
    await render("providers");

    expect(container.textContent).toContain("Claude Code");
    expect(container.textContent).toContain("Codex");
    await act(async () =>
      container
        .querySelector<HTMLButtonElement>(
          '[aria-label="Rename Default account"]',
        )!
        .click(),
    );
    const defaultInput = container.querySelector<HTMLInputElement>(
      '[aria-label="Rename Claude Code account"]',
    )!;
    await act(async () => {
      Object.getOwnPropertyDescriptor(
        HTMLInputElement.prototype,
        "value",
      )!.set!.call(defaultInput, "Primary");
      defaultInput.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await act(async () =>
      container
        .querySelector<HTMLButtonElement>('button[type="submit"]')!
        .click(),
    );
    expect(providerAccounts("claude")[0]?.label).toBe("Primary");

    await act(async () =>
      container
        .querySelector<HTMLButtonElement>('[aria-label="Rename Wrk"]')!
        .click(),
    );
    const input = container.querySelector<HTMLInputElement>(
      '[aria-label="Rename Codex account"]',
    )!;
    await act(async () => {
      Object.getOwnPropertyDescriptor(
        HTMLInputElement.prototype,
        "value",
      )!.set!.call(input, "Work");
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await act(async () =>
      container
        .querySelector<HTMLButtonElement>('button[type="submit"]')!
        .click(),
    );
    expect(providerAccounts("codex")[1]?.label).toBe("Work");

    await act(async () =>
      container
        .querySelector<HTMLButtonElement>('[aria-label="Remove Work"]')!
        .click(),
    );
    expect(ask).toHaveBeenCalled();
    expect(invoke).toHaveBeenCalledWith("provider_account_remove", {
      provider: "codex",
      accountId: "account-work",
    });
    expect(providerAccounts("codex")).toHaveLength(1);
  });

  it("reopens, scrolls to, focuses and highlights the same project on a repeated notification settings request", async () => {
    vi.useFakeTimers();
    const scroll = vi.spyOn(HTMLElement.prototype, "scrollIntoView");
    rememberNotificationProjects([
      {
        id: "repository:github.com/work/app",
        name: "work/app",
        detail: "github.com",
        kind: "repository",
        paths: ["/repo"],
      },
    ]);
    const shortcut = {
      anchor: "project-notifications" as const,
      notificationProjectPath: "/repo",
      notificationSettingsRequest: 1,
    };
    await render("inbox", shortcut);
    const project = container.querySelector<HTMLButtonElement>(
      'button[aria-label="Notification categories for work/app"]',
    )!;
    const card = project.closest("fieldset")!;
    const section = container.querySelector(
      '[data-setting-id="project-notifications"]',
    )!;
    const highlight = () => section.querySelector(".border-accent\\/60");
    expect(highlight()).not.toBeNull();
    expect(project.getAttribute("aria-expanded")).toBe("true");
    expect(document.activeElement).toBe(card);

    await act(async () => vi.advanceTimersByTimeAsync(1800));
    expect(highlight()).toBeNull();
    await act(async () => project.click());
    expect(project.getAttribute("aria-expanded")).toBe("false");
    const search = container.querySelector<HTMLInputElement>(
      '[aria-label="Search settings"]',
    )!;
    search.focus();
    expect(document.activeElement).toBe(search);
    scroll.mockClear();

    await render("inbox", { ...shortcut, notificationSettingsRequest: 2 });

    expect.soft(project.getAttribute("aria-expanded")).toBe("true");
    expect.soft(scroll).toHaveBeenCalledWith({ block: "nearest" });
    expect.soft(scroll.mock.contexts).toContain(card);
    expect.soft(document.activeElement === card).toBe(true);
    expect.soft(highlight()).not.toBeNull();

    await act(async () => vi.advanceTimersByTimeAsync(1800));
    expect(highlight()).toBeNull();
    expect(project.getAttribute("aria-expanded")).toBe("true");
    expect(document.activeElement).toBe(card);
  });

  it("clears the project shortcut highlight after 1.8 seconds while keeping its project open and focused", async () => {
    vi.useFakeTimers();
    rememberNotificationProjects([
      {
        id: "repository:github.com/work/app",
        name: "work/app",
        detail: "github.com",
        kind: "repository",
        paths: ["/repo"],
      },
    ]);
    await render("inbox", {
      anchor: "project-notifications",
      notificationProjectPath: "/repo",
    });
    const section = container.querySelector(
      '[data-setting-id="project-notifications"]',
    )!;
    const project = section.querySelector(
      'button[aria-label="Notification categories for work/app"]',
    )!;
    expect(section.querySelector(".border-accent\\/60")).not.toBeNull();
    expect(project.getAttribute("aria-expanded")).toBe("true");
    expect(document.activeElement).toBe(project.closest("fieldset"));

    await act(async () => vi.advanceTimersByTimeAsync(1800));

    expect(section.querySelector(".border-accent\\/60")).toBeNull();
    expect(project.getAttribute("aria-expanded")).toBe("true");
    expect(document.activeElement).toBe(project.closest("fieldset"));
  });

  it("gives every section a rail group", () => {
    const groups = new Set(SETTINGS_SECTIONS.map((section) => section.group));
    expect([...groups]).toEqual(["app", "agents", "workspace"]);
  });

  it("indexes each setting once", () => {
    const ids = SETTINGS_INDEX.map((entry) => entry.id);
    expect(ids).toEqual([...new Set(ids)]);
  });

  it("lets files open as normal top-bar tabs", async () => {
    await render("general");
    const control = container.querySelector<HTMLElement>(
      '[role="radiogroup"][aria-label="File tabs"]',
    )!;
    const [besideChat, topBar] = Array.from(
      control.querySelectorAll<HTMLButtonElement>('[role="radio"]'),
    );

    expect(besideChat?.getAttribute("aria-checked")).toBe("true");
    expect(topBar?.getAttribute("aria-checked")).toBe("false");

    await act(async () => topBar?.click());

    expect(topBar?.getAttribute("aria-checked")).toBe("true");
    expect(localStorage.getItem("monocode.fileTabMode")).toBe("workspace");
  });

  it("offers tab animations as an opt-in", async () => {
    await render("general");
    const control = container.querySelector<HTMLButtonElement>(
      '[role="switch"][aria-label="Tab animations"]',
    )!;

    expect(control.getAttribute("aria-checked")).toBe("false");
    await act(async () => control.click());
    expect(control.getAttribute("aria-checked")).toBe("true");
    expect(localStorage.getItem("monocode.tabAnimationsEnabled")).toBe("1");
  });

  it("defaults to the icon rail and lets users hide it", async () => {
    await render("appearance");
    let control = container.querySelector<HTMLElement>(
      '[role="radiogroup"][aria-label="Collapsed project rail"]',
    )!;
    let [iconRail, hidden] = Array.from(
      control.querySelectorAll<HTMLButtonElement>('[role="radio"]'),
    );

    expect(iconRail?.getAttribute("aria-checked")).toBe("true");
    expect(hidden?.getAttribute("aria-checked")).toBe("false");

    await act(async () => hidden?.click());

    expect(hidden?.getAttribute("aria-checked")).toBe("true");
    expect(localStorage.getItem("monocode.collapsedProjectRailMode")).toBe(
      "hidden",
    );

    await act(async () => root.unmount());
    root = createRoot(container);
    await render("appearance");

    control = container.querySelector<HTMLElement>(
      '[role="radiogroup"][aria-label="Collapsed project rail"]',
    )!;
    [iconRail, hidden] = Array.from(
      control.querySelectorAll<HTMLButtonElement>('[role="radio"]'),
    );
    expect(iconRail?.getAttribute("aria-checked")).toBe("false");
    expect(hidden?.getAttribute("aria-checked")).toBe("true");
  });

  it("reports collapsed project rail changes to the app shell", async () => {
    const onCollapsedProjectRailModeChange = vi.fn();
    await render("appearance", {
      collapsedProjectRailMode: "compact",
      onCollapsedProjectRailModeChange,
    });
    const control = container.querySelector<HTMLElement>(
      '[role="radiogroup"][aria-label="Collapsed project rail"]',
    )!;
    const hidden =
      control.querySelectorAll<HTMLButtonElement>('[role="radio"]')[1];

    await act(async () => hidden?.click());

    expect(onCollapsedProjectRailModeChange).toHaveBeenCalledWith("hidden");
  });

  // The search index is hand-maintained; this is what keeps it honest.
  it.each(
    [...new Set(SETTINGS_INDEX.map((entry) => entry.section))].map(
      (section) => ({ section }),
    ),
  )(
    "renders every indexed setting on the $section page",
    async ({ section }) => {
      await render(section);
      const expected = SETTINGS_INDEX.filter(
        (entry) => entry.section === section,
      ).map((entry) => entry.id);
      expect(renderedSettingIds().sort()).toEqual(expected.sort());
    },
  );

  it("only tags rows that search can find", async () => {
    for (const section of SETTINGS_SECTIONS.map((item) => item.id)) {
      if (section === "skills") continue;
      await render(section);
      for (const id of renderedSettingIds()) {
        expect(
          SETTINGS_INDEX.some((entry) => entry.id === id),
          `${section}: ${id}`,
        ).toBe(true);
      }
    }
  });
});

describe("settings search", () => {
  async function type(value: string) {
    const input = container.querySelector<HTMLInputElement>(
      '[aria-label="Search settings"]',
    )!;
    await act(async () => {
      Object.getOwnPropertyDescriptor(
        HTMLInputElement.prototype,
        "value",
      )!.set!.call(input, value);
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    return input;
  }

  function options(): HTMLButtonElement[] {
    return Array.from(
      document.querySelectorAll<HTMLButtonElement>('[role="option"]'),
    );
  }

  it("finds a setting that lives on another page", async () => {
    await render("general");
    await type("pacman");
    expect(options().map((item) => item.textContent)).toEqual([
      "Empty session gamesChat",
    ]);

    await act(async () => options()[0]!.click());
    expect(onSelectSection).toHaveBeenCalledWith("chat");
  });

  it("finds and reveals project notifications separately from global notifications", async () => {
    await render("general");
    await type("project notifications");
    expect(options().map((item) => item.textContent)).toEqual([
      "Project notificationsInbox",
    ]);

    await act(async () => options()[0]!.click());
    expect(onSelectSection).toHaveBeenCalledWith("inbox");
    await render("inbox");
    const section = container.querySelector(
      '[data-setting-id="project-notifications"]',
    );
    expect(
      section?.querySelector('[aria-label="Project notifications"]'),
    ).not.toBeNull();
    expect(section?.querySelector(".border-accent\\/60")).not.toBeNull();
  });

  // A page whose name starts with the query beats a setting that merely
  // mentions it; anything weaker loses to the settings themselves.
  it("ranks a page against the settings that mention it", async () => {
    await render("general");
    await type("archive");
    expect(
      options().map((item) => item.querySelector("span")!.textContent),
    ).toEqual(["Archive", "Show archived in the sidebar"]);

    await type("notification");
    expect(
      options().map((item) => item.querySelector("span")!.textContent),
    ).toEqual([
      "Notifications",
      "Project notifications",
      "Claude Code hooks",
      "General",
      "Inbox",
    ]);
  });

  it("closes the results without touching the page when cleared", async () => {
    await render("general");
    const input = await type("sounds");
    expect(options().length).toBeGreaterThan(0);
    await act(async () => {
      container
        .querySelector<HTMLButtonElement>(
          '[aria-label="Clear settings search"]',
        )!
        .click();
    });
    expect(options()).toHaveLength(0);
    expect(input.value).toBe("");
    expect(onSelectSection).not.toHaveBeenCalled();
  });

  it("records a custom keybinding from the key cell", async () => {
    await render("keybindings");
    const input = container.querySelector<HTMLInputElement>(
      '[aria-label="Change App: Search shortcut"]',
    )!;
    await act(async () => input.click());
    await act(async () =>
      document.body.dispatchEvent(
        new KeyboardEvent("keydown", {
          code: "KeyM",
          key: "m",
          ctrlKey: true,
          shiftKey: true,
          bubbles: true,
          cancelable: true,
        }),
      ),
    );

    expect(localStorage.getItem("monocode.keybindingOverrides")).toBe(
      '{"App: Search":{"shortcut":"Control+Shift+KeyM"}}',
    );
    expect(input.value).toBe("Ctrl+Shift+M");
  });

  it("disables and restores an individual keybinding", async () => {
    await render("keybindings");
    const input = container.querySelector<HTMLInputElement>(
      '[aria-label="Change App: Search shortcut"]',
    )!;
    await act(async () => input.click());
    await act(async () =>
      document.body.dispatchEvent(
        new KeyboardEvent("keydown", {
          code: "Backspace",
          key: "Backspace",
          bubbles: true,
          cancelable: true,
        }),
      ),
    );

    expect(localStorage.getItem("monocode.keybindingOverrides")).toBe(
      '{"App: Search":{"disabled":true}}',
    );
    expect(input.value).toBe("Disabled");

    await act(async () =>
      container
        .querySelector<HTMLButtonElement>(
          '[aria-label="Reset App: Search shortcut"]',
        )!
        .click(),
    );
    expect(localStorage.getItem("monocode.keybindingOverrides")).toBeNull();
  });

  it("reveals a setting on the current page", async () => {
    await render("general");
    await type("sounds");
    await act(async () => options()[0]!.click());
    expect(onSelectSection).not.toHaveBeenCalled();
    const row = container.querySelector('[data-setting-id="sounds"]')!;
    expect(row.className).toContain("bg-accent/10");
  });
});

describe("providers scope inheritance", () => {
  async function selectScope(label: string) {
    const trigger = container.querySelector<HTMLButtonElement>(
      '[aria-label^="Provider defaults scope"]',
    )!;
    await act(async () => trigger.click());
    const option = Array.from(
      document.querySelectorAll<HTMLButtonElement>('[role="option"]'),
    ).find((node) => node.textContent?.trim() === label);
    expect(option).toBeTruthy();
    await act(async () => option!.click());
  }

  it("inherits the global default provider and picker visibility in project scope", async () => {
    localStorage.setItem(
      "monocode.lastModel",
      JSON.stringify({ harness: "claude", model: "claude:opus-5" }),
    );
    localStorage.setItem(
      "monocode.hiddenPickerProviders",
      JSON.stringify(["cursor"]),
    );
    await render("providers");

    await selectScope("repo");

    // A project with no overrides shows the inherited global default provider.
    const claudeRow = container
      .querySelector('[aria-label^="Claude Code model"]')!
      .closest(".settings-row")!;
    const claudeDefault = Array.from(
      claudeRow.querySelectorAll<HTMLButtonElement>("button"),
    ).find((node) => node.textContent?.trim() === "Default");
    expect(claudeDefault).toBeTruthy();

    // Picker visibility also inherits the global setting.
    expect(
      container
        .querySelector<HTMLButtonElement>(
          '[aria-label="Show Claude Code in the model picker"]',
        )!
        .getAttribute("aria-checked"),
    ).toBe("true");
    const cursorToggle = container.querySelector<HTMLButtonElement>(
      '[aria-label="Show Cursor in the model picker"]',
    )!;
    expect(cursorToggle.getAttribute("aria-checked")).toBe("false");
    // Global precedence: the project toggle cannot turn a globally hidden
    // provider back on, so it is locked and explained.
    expect(cursorToggle.hasAttribute("disabled")).toBe(true);
    expect(cursorToggle.closest(".settings-row")?.textContent).toContain(
      "Hidden globally",
    );
  });
});
