// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import {
  loadNotificationPreferences,
  updateNotificationPreferences,
} from "../lib/notificationPreferences";
import { ProjectRail } from "./ProjectRail";
import { invoke } from "@tauri-apps/api/core";
import { rememberNotificationProjects } from "../lib/notificationProjects";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async () => ({
    root: "/work/private",
    commonDir: null,
    remote: "https://github.com/person/private.git",
  })),
  convertFileSrc: (path: string) => path,
}));
vi.mock("../hooks/useProjectDiffStats", () => ({
  useProjectDiffStats: () => null,
}));

let container: HTMLDivElement;
let root: Root;
beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  localStorage.clear();
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

function button(label: string) {
  const result = [...document.querySelectorAll("button")].find(
    (item) => (item.getAttribute("aria-label") ?? item.textContent) === label
      || (/^\d+ hours?$/.test(label) && item.textContent?.startsWith(`${label} (`)),
  );
  expect(result, label).toBeDefined();
  return result!;
}

it("opens path-based project actions immediately without Git discovery", async () => {
  rememberNotificationProjects([{
    id: "local:/work/private", name: "person/private",
    detail: "github.com", kind: "repository", paths: ["/work/private"],
  }]);
  await act(async () => root.render(createElement(ProjectRail, {
    cwd: "/work/private", recents: [], onSelectProject: vi.fn(), onOpenProject: vi.fn(),
  })));
  act(() => container.querySelector('button[aria-current="true"]')!.dispatchEvent(
    new MouseEvent("contextmenu", { bubbles: true, clientX: 40, clientY: 80 }),
  ));
  expect(button("Mute notifications").disabled).toBe(false);
  act(() => button("Mute notifications").click());
  act(() => button("1 hour").click());
  expect(loadNotificationPreferences()["local:/work/private"].mutedUntil).toBeGreaterThan(Date.now());
  expect(
    vi.mocked(invoke).mock.calls.some(([command]) => command === "git_notification_context"),
  ).toBe(false);
});

it("shows persisted mute status on the project and in its reopened menu", async () => {
  updateNotificationPreferences(["local:/work/private"], {
    mutedUntil: null,
  });
  await act(async () => root.render(createElement(ProjectRail, {
    cwd: "/work/private",
    recents: [],
    onSelectProject: vi.fn(),
    onOpenProject: vi.fn(),
  })));
  const indicator = container.querySelector('[role="img"][aria-label="Muted until resumed"]');
  expect(indicator).not.toBeNull();
  expect(indicator?.getAttribute("title")).toBe("Muted until resumed");
  const project = container.querySelector('button[aria-current="true"]')!;
  expect(project.getAttribute("aria-label")).toContain("Muted until resumed");
  await act(async () => project.dispatchEvent(new KeyboardEvent("keydown", {
    key: "ContextMenu", bubbles: true,
  })));
  const resume = button("Resume notifications");
  expect(resume.textContent).toContain("Muted until resumed");
  const menu = resume.closest('[role="menu"]')!;
  expect(menu.firstElementChild).toBe(resume);
  expect(resume.nextElementSibling?.getAttribute("role")).toBe("separator");
  expect(resume.nextElementSibling?.nextElementSibling?.getAttribute("aria-label")).toBe("Group name");
  expect(menu.querySelectorAll('[aria-label="Resume notifications"]')).toHaveLength(1);
  expect(button("Mute notifications").textContent).not.toContain("Muted until resumed");
  act(() => button("Resume notifications").click());
  expect(container.querySelector('[role="img"][aria-label="Muted until resumed"]')).toBeNull();
  expect(project.getAttribute("aria-label")).not.toContain("Muted");
  // Changes made elsewhere and automatic expiry update an already mounted rail.
  vi.useFakeTimers();
  const until = new Date(2030, 0, 15, 16, 30).getTime();
  vi.setSystemTime(until - 1000);
  act(() => updateNotificationPreferences(["local:/work/private"], {
    mutedUntil: until,
  }));
  const timedIndicator = container.querySelector('[role="img"][aria-label^="Muted until "]');
  expect(timedIndicator?.getAttribute("title")).toBe(
    `Muted until ${new Date(2030, 0, 15, 16, 30).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" })}`,
  );
  await act(async () => project.dispatchEvent(new KeyboardEvent("keydown", {
    key: "ContextMenu", bubbles: true,
  })));
  expect(button("Resume notifications").textContent).toContain(timedIndicator!.getAttribute("title"));
  await act(async () => vi.advanceTimersByTimeAsync(1000));
  expect(container.querySelector('[role="img"][aria-label^="Muted until "]')).toBeNull();
  expect(button("Mute notifications").textContent).not.toContain("Muted until");
  expect(document.querySelector('[aria-label="Resume notifications"]')).toBeNull();
  expect(document.querySelector('[role="menu"]')?.firstElementChild?.getAttribute("aria-label")).toBe("Group name");
});

it("mutes a repository from its project context menu", async () => {
  updateNotificationPreferences(["local:/work/private"], {
    disabled: ["issues"],
  });
  act(() =>
    root.render(
      createElement(ProjectRail, {
        cwd: "/work/private",
        recents: [],
        onSelectProject: vi.fn(),
        onOpenProject: vi.fn(),
      }),
    ),
  );
  await act(async () =>
    container.querySelector('button[aria-current="true"]')!.dispatchEvent(
      new MouseEvent("contextmenu", {
        bubbles: true,
        cancelable: true,
        clientX: 40,
        clientY: 80,
      }),
    ),
  );
  const mute = button("Mute notifications");
  expect(mute.getAttribute("aria-haspopup")).toBe("menu");
  act(() => mute.dispatchEvent(new MouseEvent("mouseover", { bubbles: true })));
  expect(document.querySelector('[role="dialog"]')).toBeNull();
  const write = vi.spyOn(localStorage, "setItem").mockImplementation(() => {
    throw new Error("Storage full");
  });
  act(() => button("8 hours").click());
  expect(document.querySelector('[role="alert"]')?.textContent).toContain(
    "Could not save",
  );
  expect(
    loadNotificationPreferences()["local:/work/private"]
      .mutedUntil,
  ).toBeUndefined();
  write.mockRestore();
  const start = Date.now();
  act(() => button("8 hours").click());
  const muted = Object.values(loadNotificationPreferences());
  expect(muted).toHaveLength(1);
  expect(muted[0].mutedUntil).toBeGreaterThanOrEqual(start + 28_800_000);
  expect(muted[0].mutedUntil).toBeLessThanOrEqual(Date.now() + 28_800_000);
  expect(muted[0].disabled).toEqual(["issues"]);
  expect(
    document.querySelector(
      '[role="dialog"][aria-label="Project notifications"]',
    ),
  ).toBeNull();
  const project = container.querySelector<HTMLElement>(
    'button[aria-current="true"]',
  )!;
  expect(document.activeElement).toBe(project);
  await act(async () =>
    project.dispatchEvent(
      new KeyboardEvent("keydown", { key: "ContextMenu", bubbles: true }),
    ),
  );
  const muteAgain = button("Mute notifications");
  act(() => muteAgain.focus());
  act(() =>
    muteAgain.dispatchEvent(
      new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true }),
    ),
  );
  expect(
    document.querySelector('[role="menu"][aria-label="Mute notifications"]'),
  ).not.toBeNull();
  act(() =>
    document.activeElement!.dispatchEvent(
      new KeyboardEvent("keydown", { key: "ArrowLeft", bubbles: true }),
    ),
  );
  expect(document.activeElement).toBe(muteAgain);
  act(() => button("Resume notifications").click());
  expect(
    loadNotificationPreferences()["local:/work/private"],
  ).toEqual({ disabled: ["issues"], resumedAt: expect.any(Number) });
  expect(document.activeElement).toBe(project);
});

it("opens notification settings for all projects from the Inbox context menu", async () => {
  const onOpenNotificationSettings = vi.fn();
  await act(async () =>
    root.render(
      createElement(ProjectRail, {
        cwd: "/work/private",
        recents: [],
        onSelectProject: vi.fn(),
        onOpenProject: vi.fn(),
        onOpenInbox: vi.fn(),
        onOpenNotificationSettings,
      }),
    ),
  );
  const inbox = container.querySelector('button[aria-label="Inbox"]')!;
  act(() =>
    inbox.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "ContextMenu",
        bubbles: true,
        cancelable: true,
      }),
    ),
  );
  act(() => button("Notification settings…").click());
  expect(onOpenNotificationSettings).toHaveBeenCalledWith();
  expect(
    document.querySelector('[role="menu"][aria-label="Inbox actions"]'),
  ).toBeNull();
});

it("opens project notification settings from a keyboard context menu", async () => {
  const onOpenNotificationSettings = vi.fn();
  await act(async () =>
    root.render(
      createElement(ProjectRail, {
        cwd: "/work/private",
        recents: [],
        onSelectProject: vi.fn(),
        onOpenProject: vi.fn(),
        onOpenNotificationSettings,
      }),
    ),
  );
  const project = container.querySelector<HTMLElement>(
    'button[aria-current="true"]',
  )!;
  project.focus();
  act(() =>
    project.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "F10",
        shiftKey: true,
        bubbles: true,
        cancelable: true,
      }),
    ),
  );
  act(() => button("Notification settings…").click());
  expect(onOpenNotificationSettings).toHaveBeenCalledWith("/work/private");
});
