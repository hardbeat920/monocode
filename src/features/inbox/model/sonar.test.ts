// @vitest-environment happy-dom
import { beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import {
  loadSonarIssueContent,
  peekSonarIssueContent,
  sonarBranchKey,
  sonarIssueToInboxItem,
  transitionSonarIssue,
  type SonarIssue,
  type SonarIssueContent,
} from "./sonar";
import {
  applyInboxFilters,
  DEFAULT_INBOX_FILTERS,
  hasActiveInboxFilters,
  loadInboxFilters,
  saveInboxFilters,
} from "./inboxFilters";
import {
  clearInboxCache,
  dedupeInboxItems,
  inboxItemKey,
  inboxIdentityKey,
  inboxItemRef,
  matchesInboxQuery,
  updateCachedInboxItem,
  inboxStartDraft,
  listInboxItems,
  peekInboxList,
} from "./githubTasks";
import { inboxTrackerDescription } from "./inboxContext";
import { inboxAskKey } from "./inboxAsk";
import {
  clearPendingInboxSelfActivity,
  consumeInboxSelfActivity,
} from "./inboxSelfActivity";
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

export const finding: SonarIssue = {
  key: "issue-1",
  projectKey: "org_app",
  branch: "main",
  message: "Remove unused variable",
  severity: "MINOR",
  rule: "ts:S1481",
  issueType: "CODE_SMELL",
  status: "OPEN",
  resolved: false,
  file: "src/app.ts",
  line: 12,
  updatedAt: "2026-10-14T10:00:00+0000",
  assignee: "",
  url: "https://sonarcloud.io/project/issues?id=org_app&branch=main&issues=issue-1",
};
beforeEach(() => {
  clearInboxCache();
  clearPendingInboxSelfActivity();
  localStorage.clear();
  vi.mocked(invoke).mockReset();
});

it("preserves branch and project identities even when Sonar reuses an issue key", () => {
  const main = sonarIssueToInboxItem(finding);
  const feature = sonarIssueToInboxItem({ ...finding, branch: "feature/a" });
  const other = sonarIssueToInboxItem({ ...finding, projectKey: "other_app" });
  expect(dedupeInboxItems([main, feature, other, main])).toHaveLength(3);
  expect(inboxIdentityKey(main)).toEqual(
    JSON.stringify(["org_app", "main", "issue-1"]),
  );
  expect(inboxIdentityKey(main)).not.toEqual(inboxIdentityKey(feature));
  expect(inboxItemKey(main)).not.toEqual(inboxItemKey(feature));
  expect(inboxAskKey(main)).not.toEqual(inboxAskKey(feature));
  expect(sonarBranchKey("org_app", "Main")).not.toEqual(
    sonarBranchKey("org_app", "main"),
  );
});

it("persists branch filters and applies them independently of repository and issue type filters", () => {
  const main = sonarIssueToInboxItem(finding);
  const feature = sonarIssueToInboxItem({ ...finding, branch: "feature/a" });
  const other = sonarIssueToInboxItem({ ...finding, projectKey: "other_app" });
  const filters = {
    ...DEFAULT_INBOX_FILTERS,
    hiddenSonarBranches: [sonarBranchKey("org_app", "main")],
    hiddenKinds: ["issue", "pr"] as const,
  };
  saveInboxFilters({ ...filters, hiddenKinds: [...filters.hiddenKinds] });
  const loaded = loadInboxFilters();
  expect(hasActiveInboxFilters(loaded, "sonar")).toBe(true);
  expect(hasActiveInboxFilters({ ...loaded, hiddenKinds: [] }, "github")).toBe(
    false,
  );
  expect(
    applyInboxFilters([main, feature, other], loaded, "", Date.now(), "sonar"),
  ).toEqual([feature, other]);
  expect(
    applyInboxFilters(
      [main, feature],
      DEFAULT_INBOX_FILTERS,
      "src/app.ts",
      Date.now(),
      "sonar",
    ),
  ).toHaveLength(2);
});

it("keeps scanned zero-issue branches in the cached roster even with assignee filtering", async () => {
  const branches = [
    {
      projectKey: "org_app",
      name: "main",
      isMain: true,
      analysisDate: "2026-10-14",
    },
    {
      projectKey: "org_app",
      name: "empty",
      isMain: false,
      analysisDate: "2026-10-14",
    },
  ];
  vi.mocked(invoke).mockImplementation(async (command) => {
    if (command === "sonar_status")
      return {
        connected: true,
        site: "https://sonarcloud.io",
        organization: "org",
        projects: [{ key: "org_app", name: "App" }],
        selectedProject: "org_app",
      };
    if (command === "sonar_list_issues")
      return { issues: [], branches, errors: [] };
    if (command.endsWith("_status")) return { connected: false };
    throw new Error(`Unexpected command ${command}`);
  });
  const query = { assignedToMe: true, state: "open", search: "" } as const;
  const result = await listInboxItems([], query);
  expect(result.items).toEqual([]);
  expect(result.errors).toEqual({});
  expect(result.sonarBranches).toEqual(branches);
  expect(peekInboxList([], query)?.sonarBranches).toEqual(branches);
  expect(invoke).toHaveBeenCalledWith("sonar_list_issues", {
    assignedToMe: true,
    state: "open",
  });
});

it("includes branch and source location in session and discussion context", async () => {
  vi.mocked(invoke).mockRejectedValue(new Error("Source unavailable"));
  const item = sonarIssueToInboxItem(finding);
  expect(inboxStartDraft(item)).toContain("Branch: main");
  expect(inboxStartDraft(item)).toContain("File: src/app.ts:12");
  expect(await inboxTrackerDescription(item)).toContain("Rule: ts:S1481");
  expect(invoke).toHaveBeenCalledWith("sonar_issue_content", {
    projectKey: "org_app",
    branch: "main",
    issueKey: "issue-1",
  });
});

it("does not invent a numeric reference for Sonar findings", () => {
  const item = sonarIssueToInboxItem(finding);
  expect(matchesInboxQuery(item, "#0")).toBe(false);
  expect(matchesInboxQuery(item, "0")).toBe(false);
  expect(
    matchesInboxQuery({ ...item, title: "Replace the value 0" }, "0"),
  ).toBe(true);
  expect(inboxItemRef({ ...item, identifier: undefined })).toBe("issue-1");
  expect(inboxAskKey(item)).toBe(
    `sonar:${JSON.stringify(["org_app", "main", "issue-1"])}`,
  );
});

it.each([undefined, "org_app", "new-repo"])(
  "preserves the repository name on transition response %s",
  async (repositoryName) => {
    const item = sonarIssueToInboxItem({ ...finding, repositoryName: "repo" });
    vi.mocked(invoke).mockResolvedValue({
      ...finding,
      repositoryName,
      status: "FALSE_POSITIVE",
      resolved: true,
    });
    const updated = await transitionSonarIssue(
      item,
      "falsepositive",
      "  Incorrect finding  ",
      false,
    );
    expect(invoke).toHaveBeenCalledWith("sonar_transition_issue", {
      request: {
        projectKey: "org_app",
        branch: "main",
        issueKey: "issue-1",
        transition: "falsepositive",
        comment: "Incorrect finding",
        shareFeedback: false,
      },
    });
    expect(updated.projectName).toBe(
      repositoryName === "new-repo" ? "new-repo" : "repo",
    );
    expect(updated.state).toBe("closed");
    expect(updated.sonarIssue?.status).toBe("FALSE_POSITIVE");
    expect(updated.branch).toBe("main");
  },
);

it("propagates failed transitions without changing the item", async () => {
  const item = sonarIssueToInboxItem(finding);
  vi.mocked(invoke).mockRejectedValue(new Error("Transition unavailable"));
  await expect(transitionSonarIssue(item, "accept", "", false)).rejects.toThrow(
    "Transition unavailable",
  );
  expect(item.state).toBe("open");
});

it("keeps a successful mutation cached when an older list request completes", async () => {
  const item = sonarIssueToInboxItem(finding);
  let finish: ((value: unknown) => void) | undefined;
  let pending = false;
  vi.mocked(invoke).mockImplementation(async (command) => {
    if (command === "sonar_status") return { connected: true };
    if (command === "sonar_list_issues") {
      if (pending)
        return new Promise((resolve) => {
          finish = resolve;
        });
      return { issues: [finding], branches: [], errors: [] };
    }
    if (command.endsWith("_status")) return { connected: false };
    throw new Error(command);
  });
  const query = { assignedToMe: false, state: "all", search: "" } as const;
  await listInboxItems([], query);
  pending = true;
  const refresh = listInboxItems([], query, { force: true });
  await vi.waitFor(() => expect(finish).toBeDefined());
  updateCachedInboxItem({ ...item, state: "closed" });
  finish!({ issues: [finding], branches: [], errors: [] });
  await refresh;
  expect(peekInboxList([], query)?.items[0].state).toBe("closed");
});

it("clears cached guidance and records branch-scoped self activity after a transition", async () => {
  const item = sonarIssueToInboxItem({ ...finding, repositoryName: "repo" });
  const content: SonarIssueContent = {
    repositoryName: "repo",
    issue: {},
    rule: null,
    ruleHtml: "",
    ruleUrl: "",
    referenceDocumentation: false,
    source: [],
    errors: [],
  };
  vi.mocked(invoke).mockResolvedValueOnce(content);
  await loadSonarIssueContent(item);
  expect(peekSonarIssueContent(item)).toEqual(content);
  vi.mocked(invoke).mockResolvedValueOnce({
    ...finding,
    status: "ACCEPTED",
    resolved: true,
  });
  const updated = await transitionSonarIssue(item, "accept", "", false);
  expect(peekSonarIssueContent(item)).toBeUndefined();
  expect(consumeInboxSelfActivity({ ...updated, branch: "other" })).toBe(false);
  expect(consumeInboxSelfActivity(updated)).toBe(true);
  expect(consumeInboxSelfActivity(updated)).toBe(false);
});
