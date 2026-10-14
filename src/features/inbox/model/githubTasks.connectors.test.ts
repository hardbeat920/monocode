// @vitest-environment happy-dom
import { invoke } from "@tauri-apps/api/core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  clearInboxCache,
  githubStatus,
  githubWorkItemDetails,
  inboxListCacheKey,
  listInboxItems,
  peekGithubWorkItemDetails,
  peekInboxList,
  type GithubWorkItem,
  type InboxProvider,
} from "./githubTasks";
import { setInboxConnectorEnabled } from "./inboxConnectors";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const projects = [{ path: "/tmp/web" }];
const query = { assignedToMe: false, state: "open", search: "" } as const;
const providers: InboxProvider[] = [
  "github",
  "linear",
  "jira",
  "gitlab",
  "azuredevops",
];
const item: GithubWorkItem = {
  kind: "issue",
  number: 1,
  title: "Inbox item",
  url: "https://github.com/acme/web/issues/1",
  state: "open",
  updatedAt: "2026-09-16T08:00:00Z",
  labels: [],
  assignees: [],
  draft: false,
  repo: "acme/web",
};

function disableAll() {
  for (const provider of providers) setInboxConnectorEnabled(provider, false);
}

beforeEach(() => {
  localStorage.clear();
  window.dispatchEvent(new Event("focus"));
  clearInboxCache();
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockImplementation(async (command) => {
    if (command === "git_github_repositories") return ["acme/web"];
    if (command === "git_github_work_items") return [];
    if (String(command).endsWith("_status")) return { connected: false };
    throw new Error(`Unexpected command: ${command}`);
  });
});

afterEach(() => {
  localStorage.clear();
  window.dispatchEvent(new Event("focus"));
  clearInboxCache();
});

describe("Inbox connector fetching", () => {
  it.each([
    ["github", "git_github_"],
    ["linear", "linear_"],
    ["jira", "jira_"],
    ["gitlab", "gitlab_"],
    ["azuredevops", "azure_devops_"],
  ] as const)(
    "skips %s connection checks, repository discovery and lists when disabled",
    async (provider, prefix) => {
      setInboxConnectorEnabled(provider, false);
      await expect(listInboxItems(projects, query)).resolves.toEqual({
        items: [],
        errors: {},
      });
      expect(
        vi
          .mocked(invoke)
          .mock.calls.filter(([command]) => String(command).startsWith(prefix)),
      ).toEqual([]);
    },
  );

  it("does no backend work when every connector is off", async () => {
    disableAll();
    await expect(listInboxItems(projects, query)).resolves.toEqual({
      items: [],
      errors: {},
    });
    expect(invoke).not.toHaveBeenCalled();
  });

  it("invalidates cached lists when toggled but preserves shared GitHub detail operations", async () => {
    const details = { body: "Description", author: "maya" };
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "git_github_repositories") return ["acme/web"];
      if (command === "git_github_work_items") return [item];
      if (command === "git_github_work_item_details") return details;
      return { connected: false };
    });
    await githubWorkItemDetails("/tmp/web", "acme/web", "issue", 1);
    await listInboxItems(projects, query);
    const key = inboxListCacheKey(projects, query);
    expect(peekInboxList(projects, query)?.items).toHaveLength(1);

    setInboxConnectorEnabled("github", false);
    expect(inboxListCacheKey(projects, query)).not.toBe(key);
    expect(peekInboxList(projects, query)).toBeNull();
    expect(peekGithubWorkItemDetails("acme/web", "issue", 1)).toEqual(details);
    await expect(githubStatus()).resolves.toEqual({ connected: false });

    setInboxConnectorEnabled("github", true);
    expect(peekInboxList(projects, query)).toBeNull();
    expect((await listInboxItems(projects, query)).items).toHaveLength(1);
  });

  it("does not start item lists after disabled in-flight repository discovery completes", async () => {
    let finish!: (value: string[]) => void;
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "git_github_repositories")
        return new Promise<string[]>((resolve) => {
          finish = resolve;
        });
      return { connected: false };
    });
    const pending = listInboxItems(projects, query);
    disableAll();
    finish(["acme/web"]);
    await expect(pending).resolves.toEqual({ items: [], errors: {} });
    expect(invoke).not.toHaveBeenCalledWith(
      "git_github_work_items",
      expect.anything(),
    );
    expect(peekInboxList(projects, query)).toBeNull();
  });

  it("discards stale in-flight items even if the connector is re-enabled before they finish", async () => {
    let finish!: (value: GithubWorkItem[]) => void;
    let started!: () => void;
    const began = new Promise<void>((resolve) => {
      started = resolve;
    });
    // Both kinds share one deferred response so all pending requests settle.
    const response = new Promise<GithubWorkItem[]>((resolve) => {
      finish = resolve;
    });
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "git_github_repositories") return ["acme/web"];
      if (command === "git_github_work_items") {
        started();
        return response;
      }
      return { connected: false };
    });
    const pending = listInboxItems(projects, query);
    await began;
    setInboxConnectorEnabled("github", false);
    setInboxConnectorEnabled("github", true);
    finish([item]);
    await expect(pending).resolves.toEqual({ items: [], errors: {} });
    expect(peekInboxList(projects, query)).toBeNull();
  });
});
