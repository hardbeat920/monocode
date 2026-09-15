// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import {
  loadNotificationPreferences,
  updateNotificationPreferences,
} from "../lib/notificationPreferences";
import { rememberNotificationProjects } from "../lib/notificationProjects";
import { saveNotificationsEnabled } from "../lib/notifications";
import { saveSoundsEnabled } from "../lib/sounds";
import { ProjectNotificationSettings } from "./ProjectNotificationSettings";
import { invoke } from "@tauri-apps/api/core";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockRejectedValue(new Error("No native bridge")),
}));
vi.mock("cuelume", () => ({
  play: vi.fn(),
  setEnabled: vi.fn(),
  setVolume: vi.fn(),
}));

let root: Root;
let container: HTMLDivElement;
beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  localStorage.clear();
  rememberNotificationProjects([
    {
      id: "repository:github.com/me/private",
      name: "me/private",
      detail: "github.com",
      kind: "repository",
      paths: ["/private"],
    },
    {
      id: "repository:github.com/work/app",
      name: "work/app",
      detail: "github.com",
      kind: "repository",
      paths: ["/work"],
    },
  ]);
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

function checkbox(label: string): HTMLInputElement {
  const input = container.querySelector(`input[aria-label="${label}"]`);
  expect(input, `Missing checkbox: ${label}`).toBeInstanceOf(HTMLInputElement);
  return input as HTMLInputElement;
}

function categoriesButton(project: string): HTMLButtonElement {
  return container.querySelector<HTMLButtonElement>(
    `button[aria-label="Notification categories for ${project}"]`,
  )!;
}

it("saves only the selected project's categories while preserving its mute deadline", async () => {
  updateNotificationPreferences(["repository:github.com/me/private"], {
    mutedUntil: null,
  });
  await act(async () =>
    root.render(createElement(ProjectNotificationSettings, { cwd: "" })),
  );

  act(() => categoriesButton("me/private").click());
  for (const category of [
    "Issues and Linear tasks",
    "Agent finished",
    "Agent approvals and questions",
  ]) {
    act(() => checkbox(`${category} for me/private`).click());
  }

  expect(
    checkbox("Pull requests / Merge requests for me/private").checked,
  ).toBe(true);
  expect(checkbox("Issues and Linear tasks for me/private").checked).toBe(
    false,
  );
  expect(
    loadNotificationPreferences()["repository:github.com/me/private"],
  ).toEqual({
    disabled: ["issues", "agentFinished", "agentInput"],
    mutedUntil: null,
  });
  expect(
    loadNotificationPreferences()["repository:github.com/work/app"],
  ).toBeUndefined();
});

it("offers only issue notifications for a Linear project", async () => {
  rememberNotificationProjects([
    {
      id: "linear:project:roadmap",
      name: "Roadmap",
      detail: "Linear",
      kind: "linear",
      paths: [],
    },
  ]);
  await act(async () =>
    root.render(createElement(ProjectNotificationSettings, { cwd: "" })),
  );
  act(() => categoriesButton("Roadmap").click());
  const issue = checkbox("Issues and Linear tasks for Roadmap");
  expect(
    issue
      .closest("fieldset")
      ?.querySelectorAll('input[aria-label$=" for Roadmap"]'),
  ).toHaveLength(1);
  act(() => issue.click());
  expect(
    loadNotificationPreferences()["linear:project:roadmap"]?.disabled,
  ).toEqual(["issues"]);
});

it("discovers recent projects and focuses the project requested by a quick action", async () => {
  vi.mocked(invoke)
    .mockImplementationOnce(async () => ({
      root: "/newwork",
      commonDir: null,
      remote: null,
    }))
    .mockImplementationOnce(async () => ({
      root: "/newprivate",
      commonDir: null,
      remote: null,
    }));
  await act(async () =>
    root.render(
      createElement(ProjectNotificationSettings, {
        cwd: "/newwork",
        recents: [{ path: "/newprivate", openedAt: 1 }],
        notificationProjectPath: "/newprivate",
      }),
    ),
  );
  expect(checkbox("Agent finished for newwork").checked).toBe(true);
  const target = checkbox("Agent finished for newprivate").closest("fieldset");
  expect(document.activeElement).toBe(target);
  expect(categoriesButton("newprivate").getAttribute("aria-expanded")).toBe(
    "true",
  );
  expect(categoriesButton("newwork").getAttribute("aria-expanded")).toBe(
    "false",
  );
});

it("keeps projects collapsed until opened and preserves choices when switching projects", async () => {
  await act(async () =>
    root.render(createElement(ProjectNotificationSettings, { cwd: "" })),
  );
  const personal = categoriesButton("me/private");
  const work = categoriesButton("work/app");
  const personalPanel = document.getElementById(
    personal.getAttribute("aria-controls")!,
  )!;
  const workPanel = document.getElementById(
    work.getAttribute("aria-controls")!,
  )!;
  expect(personalPanel.hidden).toBe(true);
  expect(workPanel.hidden).toBe(true);

  act(() => personal.click());
  expect(personalPanel.hidden).toBe(false);
  act(() => checkbox("Issues and Linear tasks for me/private").click());
  expect(personal.textContent).toContain("4 of 5 enabled");
  act(() => work.click());
  expect(personalPanel.hidden).toBe(true);
  expect(workPanel.hidden).toBe(false);
  act(() => personal.click());
  expect(checkbox("Issues and Linear tasks for me/private").checked).toBe(
    false,
  );
  act(() => personal.click());
  expect(personalPanel.hidden).toBe(true);
  expect(workPanel.hidden).toBe(true);
});

it("explains globally disabled channels and updates when they are enabled", async () => {
  saveSoundsEnabled(false);
  saveNotificationsEnabled(false);
  await act(async () =>
    root.render(createElement(ProjectNotificationSettings, { cwd: "" })),
  );
  expect(container.textContent).toContain("Sounds are off globally.");
  expect(container.textContent).toContain(
    "Desktop notifications are off globally.",
  );
  act(() => {
    saveSoundsEnabled(true);
    saveNotificationsEnabled(true);
  });
  expect(container.textContent).not.toContain("Sounds are off globally.");
  expect(container.textContent).not.toContain(
    "Desktop notifications are off globally.",
  );
});

it("mutes several selected projects without changing another project's notifications", async () => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date("2026-09-14T08:00:00Z"));
  rememberNotificationProjects([
    {
      id: "repository:github.com/me/other",
      name: "me/other",
      detail: "github.com",
      kind: "repository",
      paths: ["/other"],
    },
  ]);
  await act(async () =>
    root.render(createElement(ProjectNotificationSettings, { cwd: "" })),
  );
  expect(
    container.querySelector('input[aria-label="Select me/private"]'),
  ).toBeNull();
  const selectProjects = [...container.querySelectorAll("button")].find(
    (button) => button.textContent === "Select projects",
  );
  expect(selectProjects).toBeInstanceOf(HTMLButtonElement);
  act(() => selectProjects!.click());
  act(() => checkbox("Select me/private").click());
  act(() => checkbox("Select me/other").click());
  const bulk = container.querySelector('[aria-label="Mute selected projects"]');
  act(() =>
    (
      bulk?.querySelector(
        '[aria-label="Mute notifications"]',
      ) as HTMLButtonElement
    ).click(),
  );
  const eightHours = [...document.querySelectorAll('[role="menuitem"]')].find(
    (button) => button.textContent?.startsWith("8 hours ("),
  );
  expect(eightHours).toBeInstanceOf(HTMLButtonElement);
  act(() => (eightHours as HTMLButtonElement).click());
  expect(
    loadNotificationPreferences()["repository:github.com/me/private"]
      ?.mutedUntil,
  ).toBe(Date.parse("2026-09-14T16:00:00Z"));
  expect(
    loadNotificationPreferences()["repository:github.com/me/other"]?.mutedUntil,
  ).toBe(Date.parse("2026-09-14T16:00:00Z"));
  expect(
    loadNotificationPreferences()["repository:github.com/work/app"],
  ).toBeUndefined();
});

it.each(["manual", "expiry"])(
  "explains the project-wide pause and preserves editable category choices after %s resume",
  async (resumeMode) => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2030-01-15T12:00:00Z"));
    updateNotificationPreferences(["repository:github.com/me/private"], {
      mutedUntil: resumeMode === "manual" ? null : Date.now() + 60_000,
      disabled: ["issues"],
    });
    await act(async () =>
      root.render(createElement(ProjectNotificationSettings, { cwd: "" })),
    );
    const project = checkbox("Issues and Linear tasks for me/private").closest(
      "fieldset",
    )!;
    act(() => categoriesButton("me/private").click());
    expect(categoriesButton("me/private").textContent).toContain(
      "All notifications paused",
    );
    const pr = checkbox("Pull requests / Merge requests for me/private");
    const description = document.getElementById(
      pr.getAttribute("aria-describedby") ?? "",
    );
    expect(description?.textContent).toContain(
      "Your category choices apply when notifications resume.",
    );
    expect(pr.checked).toBe(true);
    expect(pr.disabled).toBe(false);
    act(() => checkbox("Reminders for me/private").click());
    expect(categoriesButton("me/private").textContent).toContain(
      "All notifications paused",
    );
    const resume = [...project.querySelectorAll("button")].find(
      (button) => button.textContent === "Resume notifications",
    );
    expect(resume).toBeInstanceOf(HTMLButtonElement);
    if (resumeMode === "manual") {
      act(() => resume!.click());
    } else {
      await act(async () => vi.advanceTimersByTimeAsync(60_000));
    }
    expect(categoriesButton("me/private").textContent).toContain(
      "3 of 5 enabled",
    );
    expect(project.textContent).not.toContain("All notifications paused");
    expect(pr.hasAttribute("aria-describedby")).toBe(false);
    expect(checkbox("Issues and Linear tasks for me/private").checked).toBe(
      false,
    );
    expect(
      loadNotificationPreferences()["repository:github.com/me/private"],
    ).toMatchObject({ disabled: ["issues", "reminders"] });
  },
);

it("dismisses the mute menu and custom date picker without changing preferences", async () => {
  await act(async () =>
    root.render(createElement(ProjectNotificationSettings, { cwd: "" })),
  );
  const project = checkbox("Issues and Linear tasks for me/private").closest(
    "fieldset",
  )!;
  const trigger = project.querySelector<HTMLButtonElement>(
    '[aria-label="Mute notifications"]',
  )!;
  act(() => trigger.click());
  expect(trigger.getAttribute("aria-expanded")).toBe("true");
  act(() =>
    window.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true }),
    ),
  );
  expect(document.querySelector('[role="menu"]')).toBeNull();
  expect(document.activeElement).toBe(trigger);

  act(() => trigger.click());
  const custom = [
    ...document.querySelectorAll<HTMLButtonElement>('[role="menuitem"]'),
  ].find((button) => button.textContent === "Choose date and time")!;
  act(() => custom.click());
  const dialog = document.querySelector(
    '[role="dialog"][aria-label="Mute project notifications"]',
  )!;
  expect(dialog).not.toBeNull();
  const cancel = [...dialog.querySelectorAll("button")].find(
    (button) => button.textContent === "Cancel",
  )!;
  act(() => cancel.click());
  expect(document.querySelector('[role="dialog"]')).toBeNull();
  expect(trigger.getAttribute("aria-expanded")).toBe("false");
  expect(document.activeElement).toBe(trigger);
  expect(
    loadNotificationPreferences()["repository:github.com/me/private"],
  ).toBeUndefined();
});
