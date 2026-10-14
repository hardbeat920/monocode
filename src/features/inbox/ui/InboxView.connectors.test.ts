// @vitest-environment happy-dom
import { invoke } from "@tauri-apps/api/core";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { clearInboxCache } from "../model/githubTasks";
import { setInboxConnectorEnabled } from "../model/inboxConnectors";
import { saveInboxConnections, saveInboxSource } from "../model/inboxFilters";
import { InboxView } from "./InboxView";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("../../../app/shell/WindowControls", () => ({
  WindowControls: () => null,
}));

let root: Root;
let container: HTMLDivElement;
const onOpenIntegrations = vi.fn();

async function mount(target?: {
  repo: string;
  kind: "issue";
  number: number;
  url: string;
}) {
  await act(async () => {
    root.render(
      createElement(InboxView, {
        cwd: "/tmp/web",
        recents: [],
        onOpenIntegrations,
        onAsk: async () => "",
        onAskRestart: async () => "",
        onAskMount: () => {},
        target,
      }),
    );
  });
}

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  localStorage.clear();
  window.dispatchEvent(new Event("focus"));
  clearInboxCache();
  onOpenIntegrations.mockReset();
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockImplementation(async (command) => {
    if (command === "git_github_repositories") return ["acme/web"];
    if (command === "git_github_work_items") return [];
    if (command === "git_github_status")
      return { connected: true, installed: true, authenticated: true };
    if (String(command).endsWith("_status")) return { connected: false };
    if (command === "linear_list_teams" || command === "jira_list_projects")
      return [];
    throw new Error(`Unexpected command: ${command}`);
  });
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  localStorage.clear();
  window.dispatchEvent(new Event("focus"));
  clearInboxCache();
  vi.unstubAllGlobals();
});

describe("Inbox connector controls", () => {
  it("omits disabled connectors from the tabs and Add connection menu", async () => {
    setInboxConnectorEnabled("jira", false);
    setInboxConnectorEnabled("gitlab", false);
    await mount();
    expect(
      [...container.querySelectorAll('[role="tab"]')].map(
        (tab) => tab.textContent,
      ),
    ).toEqual(["GitHub"]);
    await act(async () => {
      (
        container.querySelector(
          '[aria-label="Connect an inbox source"]',
        ) as HTMLButtonElement
      ).click();
    });
    expect(
      [...document.querySelectorAll('[role="menuitem"]')].map(
        (row) => row.textContent,
      ),
    ).toEqual(["Connect Linear", "Connect ADO"]);
    expect(
      vi
        .mocked(invoke)
        .mock.calls.filter(([command]) =>
          /^(jira|gitlab)_/.test(String(command)),
        ),
    ).toEqual([]);
  });

  it("resolves a disabled selected provider to an enabled source without metadata or exact-target fetching", async () => {
    saveInboxConnections({
      github: true,
      linear: true,
      jira: false,
      gitlab: false,
      azuredevops: false,
    });
    saveInboxSource("github");
    setInboxConnectorEnabled("github", false);
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "linear_status") return { connected: true };
      if (command === "linear_list_issues" || command === "linear_list_teams")
        return [];
      return { connected: false };
    });
    await mount({
      repo: "acme/web",
      kind: "issue",
      number: 42,
      url: "https://github.com/acme/web/issues/42",
    });
    expect(
      container.querySelector('[role="tab"][aria-selected="true"]')
        ?.textContent,
    ).toBe("Linear");
    expect(
      vi
        .mocked(invoke)
        .mock.calls.filter(([command]) =>
          String(command).startsWith("git_github_"),
        ),
    ).toEqual([]);
  });

  it("shows Inbox Settings when all connectors are off and makes no discovery requests", async () => {
    for (const provider of [
      "github",
      "linear",
      "jira",
      "gitlab",
      "azuredevops",
    ] as const)
      setInboxConnectorEnabled(provider, false);
    await mount();
    expect(container.querySelector('[role="tab"]')).toBeNull();
    expect(
      container.querySelector('[aria-label="Connect an inbox source"]'),
    ).toBeNull();
    const settings = [...container.querySelectorAll("button")].find(
      (button) => button.textContent === "Open Inbox Settings",
    );
    expect(settings).toBeDefined();
    await act(async () => settings!.click());
    expect(onOpenIntegrations).toHaveBeenCalledWith("github");
    expect(invoke).not.toHaveBeenCalled();
  });
});
