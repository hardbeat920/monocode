// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { InboxView } from "./InboxView";
import type { InboxListResult } from "../model/githubTasks";
import { saveInboxConnections, saveInboxSource } from "../model/inboxFilters";
import {
  sonarConnected,
  sonarIssueToInboxItem,
  type SonarStatus,
} from "../model/sonar";

const { listInboxItems } = vi.hoisted(() => ({ listInboxItems: vi.fn() }));
vi.mock("../model/githubTasks", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../model/githubTasks")>()),
  listInboxItems,
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    isMaximized: async () => false,
    onResized: async () => () => {},
  }),
}));

const PROJECTS = [
  { key: "org_app", name: "Desktop app" },
  { key: "org_api", name: "Payments API" },
];
let status: SonarStatus;
let root: Root;
let container: HTMLDivElement;

const issue = (projectKey: string, message: string) =>
  sonarIssueToInboxItem({
    key: `${projectKey}-1`,
    projectKey,
    branch: "main",
    message,
    severity: "MAJOR",
    rule: "ts:S1",
    issueType: "CODE_SMELL",
    status: "OPEN",
    resolved: false,
    file: "src/a.ts",
    line: 1,
    updatedAt: "2026-10-14T10:00:00Z",
    assignee: "",
    url: "https://sonarcloud.io/project/issues?id=x",
  });
const listed = (...items: ReturnType<typeof issue>[]): InboxListResult => ({
  items,
  errors: {},
  sonarBranches: [],
});

beforeEach(async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
  localStorage.clear();
  status = {
    connected: true,
    site: "https://sonarcloud.io",
    organization: "org",
    projects: PROJECTS,
    selectedProject: "org_app",
  };
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockImplementation(async (command, args) => {
    if (command === "sonar_status") return status;
    if (command === "sonar_select_project") {
      const { projectKey } = args as { projectKey: string };
      return (status = { ...status, selectedProject: projectKey });
    }
    throw new Error("No native bridge");
  });
  // The inbox seeds its project row from the last status read, as it does after any earlier visit.
  await sonarConnected();
  listInboxItems.mockReset();
  saveInboxConnections({
    github: false,
    linear: false,
    jira: false,
    gitlab: false,
    azuredevops: false,
    sonar: true,
  });
  saveInboxSource("sonar");
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
  document.body
    .querySelectorAll("[data-dialog-popover]")
    .forEach((element) => element.parentElement?.remove());
  localStorage.clear();
  vi.unstubAllGlobals();
});

const render = () =>
  act(async () =>
    root.render(
      createElement(InboxView, {
        cwd: "/tmp/app",
        recents: [],
        onAsk: async () => "",
        onAskRestart: async () => "",
        onAskMount: () => {},
        onOpenIntegrations: () => {},
      }),
    ),
  );
const projectButton = () =>
  container.querySelector<HTMLButtonElement>(
    'button[aria-label^="Sonar project"]',
  );
async function choose(name: string) {
  await act(async () => projectButton()!.click());
  const option = Array.from(
    document.body.querySelectorAll<HTMLButtonElement>('[role="option"]'),
  ).find((entry) => entry.textContent === name)!;
  await act(async () => option.click());
}

it("shows the selected project, or where to choose projects when none are watched", async () => {
  listInboxItems.mockResolvedValue(listed(issue("org_app", "App finding")));
  await render();
  expect(projectButton()?.title).toBe("Desktop app");
  expect(container.textContent).toContain("App finding");

  act(() => root.unmount());
  root = createRoot(container);
  status = { ...status, projects: [], selectedProject: "" };
  await sonarConnected();
  listInboxItems.mockResolvedValue(listed());
  await render();
  expect(projectButton()).toBeNull();
  expect(container.textContent).toContain(
    "Choose projects to watch in Settings → Sonar",
  );
});

it("hides the old project's issues behind a spinner until the new project loads", async () => {
  listInboxItems.mockResolvedValue(listed(issue("org_app", "App finding")));
  await render();
  let finish!: (result: InboxListResult) => void;
  listInboxItems.mockReturnValue(
    new Promise<InboxListResult>((resolve) => (finish = resolve)),
  );
  let saved!: () => void;
  const base = vi.mocked(invoke).getMockImplementation()!;
  vi.mocked(invoke).mockImplementation(async (command, args) => {
    if (command === "sonar_select_project")
      await new Promise<void>((resolve) => (saved = resolve));
    return base(command, args);
  });
  await choose("Payments API");
  expect(invoke).toHaveBeenCalledWith("sonar_select_project", {
    projectKey: "org_api",
  });
  // Before the backend confirms, the row has switched and the old issues are gone.
  expect(projectButton()?.title).toBe("Payments API");
  expect(container.textContent).not.toContain("App finding");
  expect(container.querySelector(".animate-spin")).not.toBeNull();
  expect(container.textContent).not.toContain("No Sonar issues");
  await act(async () => saved());
  expect(container.textContent).not.toContain("App finding");
  await act(async () => finish(listed(issue("org_api", "API finding"))));
  expect(container.textContent).toContain("API finding");
});

it("restores the previous project when the switch cannot be saved", async () => {
  listInboxItems.mockResolvedValue(listed(issue("org_app", "App finding")));
  await render();
  const base = vi.mocked(invoke).getMockImplementation()!;
  vi.mocked(invoke).mockImplementation(async (command, args) => {
    if (command === "sonar_select_project")
      throw new Error("Could not save Sonar settings");
    return base(command, args);
  });
  await choose("Payments API");
  expect(projectButton()?.title).toBe("Desktop app");
  expect(container.textContent).toContain("App finding");
  expect(container.textContent).toContain("Could not save Sonar settings");
  expect(container.querySelector(".animate-spin")).toBeNull();
});
