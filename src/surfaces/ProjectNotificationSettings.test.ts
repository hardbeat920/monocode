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

it("saves only the selected project's categories while preserving its mute deadline", async () => {
  updateNotificationPreferences(["repository:github.com/me/private"], {
    mutedUntil: null,
  });
  await act(async () =>
    root.render(createElement(ProjectNotificationSettings, { cwd: "" })),
  );

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
  vi.mocked(invoke).mockImplementationOnce(async () => ({
    root: "/newwork", commonDir: null, remote: null,
  })).mockImplementationOnce(async () => ({
    root: "/newprivate", commonDir: null, remote: null,
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
  act(() => checkbox("Select me/private").click());
  act(() => checkbox("Select me/other").click());
  const bulk = container.querySelector('[aria-label="Mute selected projects"]');
  const eightHours = [...(bulk?.querySelectorAll("button") ?? [])].find(
    (button) => button.textContent === "8 hours",
  );
  expect(eightHours).toBeInstanceOf(HTMLButtonElement);
  act(() => eightHours!.click());
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

it("shows a project's mute and lets it resume with its category choices intact", async () => {
  updateNotificationPreferences(["repository:github.com/me/private"], {
    mutedUntil: null,
    disabled: ["issues"],
  });
  await act(async () =>
    root.render(createElement(ProjectNotificationSettings, { cwd: "" })),
  );
  const project = checkbox("Issues and Linear tasks for me/private").closest(
    "fieldset",
  )!;
  expect(project.textContent).toContain("Muted until resumed");
  const resume = [...project.querySelectorAll("button")].find(
    (button) => button.textContent === "Resume notifications",
  );
  expect(resume).toBeInstanceOf(HTMLButtonElement);
  act(() => resume!.click());
  expect(project.textContent).not.toContain("Muted until resumed");
  expect(checkbox("Issues and Linear tasks for me/private").checked).toBe(
    false,
  );
  expect(
    loadNotificationPreferences()["repository:github.com/me/private"],
  ).toMatchObject({ disabled: ["issues"] });
});
