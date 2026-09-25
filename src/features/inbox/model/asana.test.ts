// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import {
  ASANA_CHANGE_EVENT,
  asanaIssueComment,
  asanaIssueDetails,
  asanaIssueThread,
  asanaProjectIdsForFetch,
  clearAsanaCache,
  asanaProjectIdsLinkedTo,
  disconnectAsana,
  linkAsanaProject,
  loadAsanaProjectLinks,
  loadHiddenAsanaProjectIds,
  peekAsanaIssueDetails,
  peekAsanaIssueThread,
  saveAsanaProjectLinks,
  saveAsanaToken,
  saveHiddenAsanaProjectIds,
  type AsanaIssue,
} from "./asana";
import {
  clearInboxCache,
  inboxItemRef,
  inboxItemStatus,
  inboxStartDraft,
  listInboxItems,
} from "./githubTasks";
import { inboxTrackerDescription } from "./inboxContext";
import {
  clearPendingInboxSelfActivity,
  consumeInboxSelfActivity,
} from "./inboxSelfActivity";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const issue: AsanaIssue = {
  provider: "asana",
  kind: "asana",
  id: "1200000000000042",
  identifier: "1200000000000042",
  number: 1200000000000042,
  title: "Fix auth",
  url: "https://app.asana.com/0/1200000000000001/1200000000000042",
  state: "Open",
  stateType: "new",
  updatedAt: "2026-09-23T10:00:00Z",
  dueOn: "",
  labels: [],
  assignees: [],
  draft: false,
  repo: "Acme",
  teamId: "1200000000000001",
  teamName: "Launch",
  projectPath: "",
};
const projects = [
  { id: "1200000000000001", key: "Acme", name: "Launch" },
  { id: "1200000000000002", key: "Acme", name: "Support" },
];
const query = { assignedToMe: true, state: "open", search: "" } as const;

beforeEach(() => {
  clearInboxCache();
  clearPendingInboxSelfActivity();
  localStorage.clear();
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockImplementation(async (command) => {
    if (command === "asana_status") return { connected: true };
    if (command.endsWith("_status")) return { connected: false };
    if (command === "asana_list_projects") return projects;
    if (command === "asana_list_issues") return [issue];
    if (command === "asana_issue_details")
      return { body: "Reproduction steps", author: "Ada" };
    if (command === "asana_issue_thread")
      return { comments: [], truncated: false };
    if (command === "asana_issue_comment") return "1200000000000099";
    if (command === "asana_set_token")
      return { connected: false, name: "", email: "" };
    throw new Error(`Unexpected command: ${command}`);
  });
});

describe("Asana inbox", () => {
  it("loads account-wide tasks without a local repository", async () => {
    const result = await listInboxItems([], query);
    expect(result).toEqual({ items: [issue], errors: {} });
    expect(invoke).toHaveBeenCalledWith("asana_list_issues", {
      assignedToMe: true,
      state: "open",
      projectIds: [],
      limit: 500,
    });
    expect(inboxItemStatus(issue)).toBe("Open");
    expect(inboxItemStatus({ ...issue, stateType: "done" })).toBe("Closed");
    expect(inboxItemRef(issue)).toBe("1200000000000042");
  });

  it("fetches only incomplete tasks, even when closed history is requested", async () => {
    await listInboxItems([], { ...query, state: "all" });
    expect(invoke).toHaveBeenCalledWith(
      "asana_list_issues",
      expect.objectContaining({ state: "open", limit: 500 }),
    );
  });

  it("filters projects before fetching and skips when every project is hidden", async () => {
    await listInboxItems([], {
      ...query,
      asanaHiddenProjectIds: ["1200000000000002"],
    });
    expect(invoke).toHaveBeenCalledWith(
      "asana_list_issues",
      expect.objectContaining({ projectIds: ["1200000000000001"] }),
    );
    vi.mocked(invoke).mockClear();
    expect(
      (
        await listInboxItems([], {
          ...query,
          asanaHiddenProjectIds: ["1200000000000001", "1200000000000002"],
        })
      ).items,
    ).toEqual([]);
    expect(invoke).not.toHaveBeenCalledWith(
      "asana_list_issues",
      expect.anything(),
    );
    expect(asanaProjectIdsForFetch(projects, ["deleted"])).toBeNull();
    expect(asanaProjectIdsForFetch(projects, [])).toBeNull();
  });

  it("drops tasks from hidden projects that the fetch still returned", async () => {
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "asana_status") return { connected: true };
      if (command.endsWith("_status")) return { connected: false };
      if (command === "asana_list_projects") return [];
      if (command === "asana_list_issues") return [issue];
      throw new Error(`Unexpected command: ${command}`);
    });
    const result = await listInboxItems([], {
      ...query,
      asanaHiddenProjectIds: [issue.teamId],
    });
    expect(result.items).toEqual([]);
  });

  it("keeps GitHub items when Asana settings cannot be read", async () => {
    const original = vi.mocked(invoke).getMockImplementation()!;
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === "asana_status")
        throw new Error("Asana settings are invalid");
      if (command === "git_github_repositories") return ["acme/web"];
      if (command === "git_github_work_items") {
        return (args as { kind: string }).kind === "issue"
          ? [{ ...issue, kind: "issue", repo: "acme/web" }]
          : [];
      }
      return original(command, args);
    });
    const result = await listInboxItems([{ path: "/repo" }], query);
    expect(result.items).toHaveLength(1);
    expect(result.items[0].provider).toBe("github");
    expect(result.errors).toEqual({ asana: "Asana settings are invalid" });
  });

  it("provides the Asana description and task id to sessions", async () => {
    const description = await inboxTrackerDescription(issue);
    expect(invoke).toHaveBeenCalledWith("asana_issue_details", {
      id: "1200000000000042",
    });
    const draft = inboxStartDraft(issue, description);
    expect(draft).toContain("Work on this Asana task:");
    expect(draft).toContain("1200000000000042 Fix auth");
    expect(draft).toContain("Reproduction steps");
    vi.mocked(invoke).mockClear();
    expect(await inboxTrackerDescription(issue, "Provided description")).toBe(
      "Provided description",
    );
    expect(invoke).not.toHaveBeenCalled();
    await expect(
      inboxTrackerDescription({ ...issue, id: "" }),
    ).rejects.toThrow("Missing Asana task");
  });

  it("invalidates comments and suppresses notifications for the author's own comment", async () => {
    await asanaIssueThread(issue.id);
    expect(peekAsanaIssueThread(issue.id)).not.toBeNull();
    await expect(
      asanaIssueComment(issue.id, "  Fixed\n\nPlease check  "),
    ).resolves.toBe("1200000000000099");
    expect(invoke).toHaveBeenCalledWith("asana_issue_comment", {
      id: "1200000000000042",
      body: "Fixed\n\nPlease check",
    });
    expect(peekAsanaIssueThread(issue.id)).toBeNull();
    expect(consumeInboxSelfActivity(issue)).toBe(true);
    expect(consumeInboxSelfActivity(issue)).toBe(false);
  });

  it("clears the token and cached descriptions on disconnect and reconnect", async () => {
    await asanaIssueDetails(issue.id);
    await disconnectAsana();
    expect(invoke).toHaveBeenCalledWith("asana_set_token", { token: "" });
    expect(peekAsanaIssueDetails(issue.id)).toBeNull();
    await asanaIssueDetails(issue.id);
    await saveAsanaToken(" token ");
    expect(invoke).toHaveBeenCalledWith("asana_set_token", { token: "token" });
    expect(peekAsanaIssueDetails(issue.id)).toBeNull();
  });

  it("does not restore a previous account's cache when a request finishes late", async () => {
    let resolve!: (value: unknown) => void;
    vi.mocked(invoke).mockReturnValueOnce(
      new Promise((done) => {
        resolve = done;
      }),
    );
    const pending = asanaIssueDetails(issue.id);
    clearAsanaCache();
    resolve({ body: "Old account", author: "Ada" });
    await pending;
    expect(peekAsanaIssueDetails(issue.id)).toBeNull();
  });

  it("persists hidden projects and announces the change", () => {
    const listener = vi.fn();
    window.addEventListener(ASANA_CHANGE_EVENT, listener);
    saveHiddenAsanaProjectIds(["1200000000000002"]);
    window.removeEventListener(ASANA_CHANGE_EVENT, listener);
    expect(listener).toHaveBeenCalledTimes(1);
    expect(loadHiddenAsanaProjectIds()).toEqual(["1200000000000002"]);
    expect(localStorage.getItem("monocode.asanaHiddenProjects")).toBe(
      '["1200000000000002"]',
    );
    localStorage.setItem("monocode.asanaHiddenProjects", "not json");
    expect(loadHiddenAsanaProjectIds()).toEqual([]);
  });

  it("always fetches My Tasks, even with assigned-to-me off", async () => {
    await listInboxItems([], { ...query, assignedToMe: false });
    expect(invoke).toHaveBeenCalledWith(
      "asana_list_issues",
      expect.objectContaining({ assignedToMe: true }),
    );
  });

  it("tags tasks with the local project their Asana project is linked to", async () => {
    saveAsanaProjectLinks({ "1200000000000002": "/work/app/" });
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "asana_status") return { connected: true };
      if (command.endsWith("_status")) return { connected: false };
      if (command === "asana_list_issues") {
        return [
          {
            ...issue,
            projects: [
              { id: "1200000000000001", name: "Launch" },
              { id: "1200000000000002", name: "Support" },
            ],
          },
          { ...issue, id: "1200000000000043", projects: [] },
        ];
      }
      throw new Error(`Unexpected command: ${command}`);
    });
    const { items } = await listInboxItems([], query);
    const byId = (id: string) => items.find((item) => item.id === id);
    expect(byId("1200000000000042")).toMatchObject({
      projectPath: "/work/app",
      teamId: "1200000000000002",
      teamName: "Support",
      repo: "Support",
    });
    expect(byId("1200000000000043")).toMatchObject({
      projectPath: "",
      teamId: "1200000000000001",
    });
  });

  it("persists project links and announces the change", () => {
    const listener = vi.fn();
    window.addEventListener(ASANA_CHANGE_EVENT, listener);
    const linked = linkAsanaProject({}, "1200000000000001", "/work/app/");
    saveAsanaProjectLinks(linkAsanaProject(linked, "1200000000000002", "/work/app"));
    window.removeEventListener(ASANA_CHANGE_EVENT, listener);
    expect(listener).toHaveBeenCalledTimes(1);
    const links = loadAsanaProjectLinks();
    expect(links).toEqual({
      "1200000000000001": "/work/app",
      "1200000000000002": "/work/app",
    });
    expect(asanaProjectIdsLinkedTo(links, "/work/app/")).toEqual([
      "1200000000000001",
      "1200000000000002",
    ]);
    expect(asanaProjectIdsLinkedTo(links, "/work/other")).toEqual([]);
    expect(linkAsanaProject(links, "1200000000000001", "")).toEqual({
      "1200000000000002": "/work/app",
    });
    localStorage.setItem("monocode.asanaProjectLinks", "[1]");
    expect(loadAsanaProjectLinks()).toEqual({});
  });
});
