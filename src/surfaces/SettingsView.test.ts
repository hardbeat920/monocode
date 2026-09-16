// @vitest-environment happy-dom
import { act, createElement, type ComponentProps } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SettingsView } from "./SettingsView";
import { rememberNotificationProjects } from "../lib/notificationProjects";
import {
  SETTINGS_INDEX,
  SETTINGS_SECTIONS,
  type SettingsSectionId,
} from "../lib/settings";

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

let container: HTMLDivElement;
let root: Root;
let onSelectSection: ReturnType<typeof vi.fn>;

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
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
  onSelectSection = vi.fn();
});

afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
  localStorage.clear();
  vi.restoreAllMocks();
  vi.useRealTimers();
});

describe("settings pages", () => {
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

  it("reveals a setting on the current page", async () => {
    await render("general");
    await type("sounds");
    await act(async () => options()[0]!.click());
    expect(onSelectSection).not.toHaveBeenCalled();
    const row = container.querySelector('[data-setting-id="sounds"]')!;
    expect(row.className).toContain("bg-accent/10");
  });
});
