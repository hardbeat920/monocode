// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import {
  loadNotificationPreferences,
  updateNotificationPreferences,
} from "../lib/notificationPreferences";
import { rememberNotificationProjects } from "../lib/notificationProjects";
import { ProjectRail } from "./ProjectRail";
import { invoke } from "@tauri-apps/api/core";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (_command: string, args: { cwd: string }) => ({
    root: args.cwd,
    commonDir: null,
    remote: args.cwd.includes("work")
      ? "https://github.com/company/work.git"
      : "https://github.com/person/private.git",
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
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

function button(label: string) {
  const result = [...document.querySelectorAll("button")].find(
    (item) => item.textContent === label
      || (/^\d+ hours?$/.test(label) && item.textContent?.startsWith(`${label} (`)),
  );
  expect(result, label).toBeDefined();
  return result!;
}

async function openInboxMenu() {
  await act(async () =>
    root.render(
      createElement(ProjectRail, {
        cwd: "/repos/work",
        recents: [{ path: "/repos/private", openedAt: 1 }],
        onSelectProject: vi.fn(),
        onOpenProject: vi.fn(),
        onOpenInbox: vi.fn(),
      }),
    ),
  );
  const inbox = container.querySelector<HTMLElement>(
    'button[aria-label="Inbox"]',
  )!;
  inbox.focus();
  await act(async () =>
    inbox.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "ContextMenu",
        bubbles: true,
        cancelable: true,
      }),
    ),
  );
  return inbox;
}

it("reopens known Inbox actions without a disabled loading frame", async () => {
  const inbox = await openInboxMenu();
  expect(button("Mute all projects").disabled).toBe(false);
  act(() => window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })));
  act(() => inbox.dispatchEvent(new KeyboardEvent("keydown", { key: "ContextMenu", bubbles: true })));
  expect(button("Mute all projects").disabled).toBe(false);
  expect(document.querySelector('[role="status"]')?.textContent).toContain("2 projects");
});

it("mutes all rail and known Inbox projects for one hour directly from Inbox", async () => {
  vi.spyOn(Date, "now").mockReturnValue(new Date(2030, 0, 15, 20, 30).getTime());
  rememberNotificationProjects([
    {
      id: "linear:project:planning",
      name: "Planning",
      detail: "Linear",
      kind: "linear",
      paths: [],
    },
  ]);
  const inbox = await openInboxMenu();
  act(() => button("Mute all projects").click());
  expect(button("1 hour").textContent).toBe("1 hour (21:30)");
  expect(button("4 hours").textContent).toBe("4 hours (Tomorrow, 0:30)");
  expect(button("8 hours").textContent).toBe("8 hours (Tomorrow, 4:30)");
  expect(button("Until resumed").textContent).toBe("Until resumed");
  expect(button("Choose date and time").textContent).toBe("Choose date and time");
  const start = Date.now();
  act(() => button("1 hour").click());
  const preferences = loadNotificationPreferences();
  expect(Object.keys(preferences).sort()).toEqual([
    "linear:project:planning",
    "repository:github.com/company/work",
    "repository:github.com/person/private",
  ]);
  for (const preference of Object.values(preferences)) {
    expect(preference.mutedUntil).toBeGreaterThanOrEqual(start + 3_600_000);
    expect(preference.mutedUntil).toBeLessThanOrEqual(Date.now() + 3_600_000);
  }
  expect(
    document.querySelector('[role="menu"][aria-label="Inbox actions"]'),
  ).toBeNull();
  expect(document.activeElement).toBe(inbox);
});

it("offers explicit all-project actions without an implicit active-project exclusion", async () => {
  rememberNotificationProjects([
    {
      id: "repository:github.com/company/work",
      name: "company/work",
      detail: "github.com",
      kind: "repository",
      paths: ["/elsewhere/work-checkout"],
    },
  ]);
  updateNotificationPreferences(["repository:github.com/company/work"], {
    disabled: ["issues"],
  });
  await openInboxMenu();
  expect(document.body.textContent).not.toContain("Mute other projects");
  expect(document.body.textContent).not.toContain("Keeps");
  act(() => button("Mute all projects").click());
  act(() => button("Until resumed").click());
  expect(loadNotificationPreferences()).toEqual({
    "repository:github.com/company/work": {
      disabled: ["issues"],
      mutedUntil: null,
    },
    "repository:github.com/person/private": { disabled: [], mutedUntil: null },
  });
});

it("resumes muted projects without changing category choices or unmuted projects", async () => {
  updateNotificationPreferences(["repository:github.com/company/work"], {
    disabled: ["issues"],
    mutedUntil: null,
  });
  updateNotificationPreferences(["repository:github.com/person/private"], {
    disabled: ["agentFinished"],
  });
  await openInboxMenu();
  act(() => button("Resume muted projects").click());
  expect(loadNotificationPreferences()).toEqual({
    "repository:github.com/company/work": {
      disabled: ["issues"],
      resumedAt: expect.any(Number),
    },
    "repository:github.com/person/private": { disabled: ["agentFinished"] },
  });
  await openInboxMenu();
  expect(button("Resume muted projects").disabled).toBe(true);
});

it("opens custom timing from the duration submenu for all projects", async () => {
  const now = vi.spyOn(Date, "now").mockReturnValue(new Date(2030, 0, 15, 9).getTime());
  await openInboxMenu();
  act(() => button("Mute all projects").click());
  act(() => button("Choose date and time").click());
  expect(document.querySelector('input[type="datetime-local"]')).toBeNull();
  expect([...document.querySelectorAll("button")].some(item => item.textContent === "1 hour")).toBe(false);
  act(() => document.querySelector<HTMLButtonElement>('button[aria-label="2030-01-16"]')!.click());
  const input = document.querySelector<HTMLInputElement>('input[placeholder="HH:mm"]');
  expect(input).not.toBeNull();
  act(() => {
    Object.getOwnPropertyDescriptor(
      HTMLInputElement.prototype,
      "value",
    )!.set!.call(input, "12:00");
    input!.dispatchEvent(new Event("input", { bubbles: true }));
  });
  act(() => button("Mute until then").click());
  expect(loadNotificationPreferences()).toEqual({
    "repository:github.com/company/work": {
      disabled: [],
      mutedUntil: new Date(2030, 0, 16, 12).getTime(),
    },
    "repository:github.com/person/private": {
      disabled: [],
      mutedUntil: new Date(2030, 0, 16, 12).getTime(),
    },
  });
  now.mockRestore();
});

it("keeps the menu open and reports failed persistence so the action can be retried", async () => {
  await openInboxMenu();
  act(() => button("Mute all projects").click());
  const write = vi.spyOn(localStorage, "setItem").mockImplementation(() => {
    throw new Error("Storage full");
  });
  act(() => button("4 hours").click());
  expect(document.querySelector('[role="alert"]')?.textContent).toContain(
    "Could not save",
  );
  expect(loadNotificationPreferences()).toEqual({});
  expect(
    document.querySelector('[role="menu"][aria-label="Inbox actions"]'),
  ).not.toBeNull();
  write.mockRestore();
  act(() => button("4 hours").click());
  expect(Object.keys(loadNotificationPreferences())).toHaveLength(2);
  expect(
    document.querySelector('[role="menu"][aria-label="Inbox actions"]'),
  ).toBeNull();
});

it("disables bulk actions on unresolved projects and allows retrying discovery", async () => {
  const resolve = vi.mocked(invoke).getMockImplementation()!;
  vi.mocked(invoke).mockResolvedValue(null);
  await openInboxMenu();
  expect(button("Mute all projects").disabled).toBe(true);
  expect(button("Resume muted projects").disabled).toBe(true);
  expect(document.querySelector('[role="alert"]')?.textContent).toContain(
    "Could not load projects",
  );
  vi.mocked(invoke).mockImplementation(resolve);
  await act(async () => button("Retry loading projects").click());
  expect(button("Mute all projects").disabled).toBe(false);
  expect(document.querySelector('[role="alert"]')).toBeNull();
});
