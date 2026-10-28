// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import {
  clearJiraCache,
  disconnectJira,
  jiraIssueDetails,
  jiraIssueThread,
  jiraIssueComment,
  jiraProjectIdsForFetch,
  peekJiraIssueDetails,
  peekJiraIssueThread,
  saveJiraConfig,
  type JiraIssue,
} from "./jira";
import {
  clearInboxCache,
  listInboxItems,
  inboxItemStatus,
  inboxStartDraft,
} from "./githubTasks";
import { inboxTrackerDescription } from "./inboxContext";
import {
  clearPendingInboxSelfActivity,
  consumeInboxSelfActivity,
} from "./inboxSelfActivity";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const issue: JiraIssue = {
  provider: "jira",
  kind: "jira",
  id: "10042",
  identifier: "ENG-42",
  number: 42,
  title: "Fix auth",
  url: "https://acme.atlassian.net/browse/ENG-42",
  state: "In Progress",
  stateType: "indeterminate",
  updatedAt: "2026-09-23T10:00:00Z",
  labels: [],
  assignees: [],
  draft: false,
  repo: "ENG",
  teamId: "10000",
  teamName: "Engineering",
  projectPath: "",
};
const projects = [
  { id: "10000", key: "ENG", name: "Engineering" },
  { id: "10001", key: "OPS", name: "Operations" },
];
const query = { assignedToMe: true, state: "open", search: "" } as const;

beforeEach(() => {
  clearInboxCache();
  clearPendingInboxSelfActivity();
  localStorage.clear();
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockImplementation(async (command) => {
    if (command === "jira_status") return { connected: true };
    if (command.endsWith("_status")) return { connected: false };
    if (command === "jira_list_projects") return projects;
    if (command === "jira_list_issues") return [issue];
    if (command === "jira_issue_details")
      return { body: "Reproduction steps", author: "Ada" };
    if (command === "jira_issue_thread")
      return { comments: [], truncated: false };
    if (command === "jira_issue_comment")
      return `${issue.url}?focusedCommentId=7`;
    if (command === "jira_set_config")
      return { connected: false, site: "", email: "" };
    throw new Error(`Unexpected command: ${command}`);
  });
});

describe("Jira inbox", () => {
  it("loads account-wide issues without a local repository", async () => {
    const result = await listInboxItems([], query);
    expect(result).toEqual({ items: [issue], errors: {} });
    expect(invoke).toHaveBeenCalledWith("jira_list_issues", {
      assignedToMe: true,
      state: "open",
      projectIds: [],
      limit: undefined,
    });
    expect(inboxItemStatus(issue)).toBe("Open");
    expect(inboxItemStatus({ ...issue, stateType: "done" })).toBe("Closed");
  });

  it("filters projects before fetching and skips when every project is hidden", async () => {
    await listInboxItems([], { ...query, jiraHiddenProjectIds: ["10001"] });
    expect(invoke).toHaveBeenCalledWith(
      "jira_list_issues",
      expect.objectContaining({ projectIds: ["10000"] }),
    );
    vi.mocked(invoke).mockClear();
    expect(
      (
        await listInboxItems([], {
          ...query,
          jiraHiddenProjectIds: ["10000", "10001"],
        })
      ).items,
    ).toEqual([]);
    expect(invoke).not.toHaveBeenCalledWith(
      "jira_list_issues",
      expect.anything(),
    );
    expect(jiraProjectIdsForFetch(projects, ["deleted"])).toBeNull();
  });

  it("keeps GitHub items when Jira settings cannot be read", async () => {
    const original = vi.mocked(invoke).getMockImplementation()!;
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === "jira_status")
        throw new Error("Jira settings are invalid");
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
    expect(result.errors).toEqual({ jira: "Jira settings are invalid" });
  });

  it("provides Jira description and identifier to sessions", async () => {
    const description = await inboxTrackerDescription(issue);
    expect(invoke).toHaveBeenCalledWith("jira_issue_details", {
      key: "ENG-42",
    });
    expect(inboxStartDraft(issue, description)).toContain("ENG-42 Fix auth");
    expect(inboxStartDraft(issue, description)).toContain("Reproduction steps");
    vi.mocked(invoke).mockClear();
    expect(await inboxTrackerDescription(issue, "Provided description")).toBe(
      "Provided description",
    );
    expect(invoke).not.toHaveBeenCalled();
    await expect(
      inboxTrackerDescription({ ...issue, identifier: "" }),
    ).rejects.toThrow("Missing Jira issue key");
  });

  it("invalidates comments and suppresses notifications for the author's own comment", async () => {
    await jiraIssueThread(issue.identifier);
    expect(peekJiraIssueThread(issue.identifier)).not.toBeNull();
    await jiraIssueComment(
      { id: issue.id, key: issue.identifier },
      "  Fixed\n\nPlease check  ",
    );
    expect(invoke).toHaveBeenCalledWith("jira_issue_comment", {
      key: "ENG-42",
      body: "Fixed\n\nPlease check",
    });
    expect(peekJiraIssueThread(issue.identifier)).toBeNull();
    expect(consumeInboxSelfActivity(issue)).toBe(true);
    expect(consumeInboxSelfActivity(issue)).toBe(false);
  });

  it("clears credentials and cached descriptions on disconnect and reconnect", async () => {
    await jiraIssueDetails(issue.identifier);
    await disconnectJira();
    expect(invoke).toHaveBeenCalledWith("jira_set_config", {
      site: "",
      email: "",
      token: "",
    });
    expect(peekJiraIssueDetails(issue.identifier)).toBeNull();
    await jiraIssueDetails(issue.identifier);
    await saveJiraConfig({
      site: " acme ",
      email: " ada@example.com ",
      token: " token ",
    });
    expect(invoke).toHaveBeenCalledWith("jira_set_config", {
      site: "acme",
      email: "ada@example.com",
      token: "token",
    });
    expect(peekJiraIssueDetails(issue.identifier)).toBeNull();
  });

  it("does not restore a previous account's cache when a request finishes late", async () => {
    let resolve!: (value: unknown) => void;
    vi.mocked(invoke).mockReturnValueOnce(
      new Promise((done) => {
        resolve = done;
      }),
    );
    const pending = jiraIssueDetails(issue.identifier);
    clearJiraCache();
    resolve({ body: "Old account", author: "Ada" });
    await pending;
    expect(peekJiraIssueDetails(issue.identifier)).toBeNull();
  });
});
