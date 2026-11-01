import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  clearInboxCache,
  githubPrAction,
  githubWorkItem,
  inboxItemKey,
  type GithubWorkItem,
  type InboxItem,
} from "../../inbox/model/githubTasks";
import {
  inboxItemMatchesLinkedWorkItem,
  linearWorkspaceFromUrl,
  linkedWorkItemInboxKey,
  linkedWorkItemFromAutomationEvent,
  linkedWorkItemFromInboxItem,
  parseGithubWorkItemUrl,
  parseLinearWorkItemUrl,
  normalizeLinkedWorkItem,
  relatedSessionsForInboxItem,
  resolveLinkedWorkItem,
  ticketKeysForTeams,
  ticketKeysInMessage,
} from "./sessionWorkItem";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

beforeEach(() => {
  clearInboxCache();
  vi.mocked(invoke).mockReset();
});

describe("session work items", () => {
  it("parses a GitHub pull request URL without repository lookup", () => {
    expect(
      parseGithubWorkItemUrl(
        "Please review https://github.com/openai/codex/pull/321?diff=split",
      ),
    ).toEqual({
      kind: "pr",
      repo: "openai/codex",
      number: 321,
      url: "https://github.com/openai/codex/pull/321",
    });
  });

  it("creates a stable link from a GitHub Inbox item", () => {
    const item = {
      provider: "github",
      kind: "issue",
      repo: "openai/codex",
      number: 12,
      url: "https://github.com/openai/codex/issues/12",
    } as InboxItem;
    const linked = linkedWorkItemFromInboxItem(item);
    expect(linked).toEqual({
      kind: "issue",
      repo: "openai/codex",
      number: 12,
      url: "https://github.com/openai/codex/issues/12",
    });
    expect(inboxItemMatchesLinkedWorkItem(item, linked!)).toBe(true);
    expect(linkedWorkItemInboxKey(linked!)).toBe(inboxItemKey(item));
  });

  it("restores a linked PR from a persisted automation event", () => {
    expect(
      linkedWorkItemFromAutomationEvent({
        trigger: "event",
        eventKind: "github",
        eventKey: "github:pr:openai/codex:321",
      }),
    ).toEqual({
      kind: "pr",
      repo: "openai/codex",
      number: 321,
      url: "https://github.com/openai/codex/pull/321",
    });
  });

  it("does not link non-GitHub or malformed automation events", () => {
    expect(
      linkedWorkItemFromAutomationEvent({
        trigger: "event",
        eventKind: "gitlab",
        eventKey: "gitlab:pr:openai/codex:321",
      }),
    ).toBeNull();
    expect(
      linkedWorkItemFromAutomationEvent({
        trigger: "event",
        eventKind: "github",
        eventKey: "github:pr:missing-number",
      }),
    ).toBeNull();
  });

  it("resolves an explicit PR number against the session repository", async () => {
    vi.mocked(invoke).mockResolvedValue("openai/codex");

    await expect(
      resolveLinkedWorkItem("Please fix PR #42", "/tmp/codex", null),
    ).resolves.toEqual({
      kind: "pr",
      repo: "openai/codex",
      number: 42,
      url: "https://github.com/openai/codex/pull/42",
    });
    expect(invoke).toHaveBeenCalledWith("git_github_repo", {
      cwd: "/tmp/codex",
    });
  });

  it("fetches an exact cache miss once and reuses that result", async () => {
    const result: GithubWorkItem = {
      kind: "pr",
      repo: "openai/codex",
      number: 42,
      title: "Faster linked navigation",
      url: "https://github.com/openai/codex/pull/42",
      state: "open",
      updatedAt: "2026-09-09T12:00:00Z",
      labels: [],
      assignees: [],
      draft: false,
    };
    vi.mocked(invoke).mockResolvedValue(result);

    await expect(
      githubWorkItem("/tmp/codex", "openai/codex", "pr", 42),
    ).resolves.toEqual(result);
    await expect(
      githubWorkItem("/tmp/codex", "openai/codex", "pr", 42),
    ).resolves.toEqual(result);

    const refreshed = { ...result, updatedAt: "2026-09-09T12:01:00Z" };
    vi.mocked(invoke).mockResolvedValueOnce(refreshed);
    await expect(
      githubWorkItem("/tmp/codex", "openai/codex", "pr", 42, {
        force: true,
      }),
    ).resolves.toEqual(refreshed);
    await expect(
      githubWorkItem("/tmp/codex", "openai/codex", "pr", 42),
    ).resolves.toEqual(refreshed);

    expect(invoke).toHaveBeenCalledTimes(2);
    expect(invoke).toHaveBeenCalledWith("git_github_work_item", {
      cwd: "/tmp/codex",
      repo: "openai/codex",
      kind: "pr",
      number: 42,
    });
  });

  it("runs a pull request action and caches the refreshed result", async () => {
    const merged: GithubWorkItem = {
      kind: "pr",
      repo: "openai/codex",
      number: 42,
      title: "Faster linked navigation",
      url: "https://github.com/openai/codex/pull/42",
      state: "merged",
      updatedAt: "2026-09-09T12:05:00Z",
      labels: [],
      assignees: [],
      draft: false,
    };
    vi.mocked(invoke).mockResolvedValue(merged);

    await expect(
      githubPrAction("/tmp/codex", "openai/codex", 42, "squash"),
    ).resolves.toEqual(merged);
    await expect(
      githubWorkItem("/tmp/codex", "openai/codex", "pr", 42),
    ).resolves.toEqual(merged);

    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith("git_github_pr_action", {
      cwd: "/tmp/codex",
      repo: "openai/codex",
      number: 42,
      action: "squash",
    });
  });

  it("creates a stable link from a Linear Inbox item", () => {
    const item = {
      provider: "linear",
      kind: "linear",
      id: "issue-uuid",
      identifier: "ENG-12",
      number: 12,
      repo: "ENG",
      url: "https://linear.app/acme/issue/ENG-12/slug",
    } as InboxItem;
    const linked = linkedWorkItemFromInboxItem(item);
    expect(linked).toEqual({
      kind: "linear",
      identifier: "ENG-12",
      id: "issue-uuid",
      repo: "ENG",
      number: 12,
      url: "https://linear.app/acme/issue/ENG-12/slug",
    });
    expect(inboxItemMatchesLinkedWorkItem(item, linked!)).toBe(true);
    expect(linkedWorkItemInboxKey(linked!)).toBe(inboxItemKey(item));
    expect(
      linkedWorkItemFromInboxItem({
        provider: "linear",
        kind: "linear",
        number: 12,
        repo: "",
      } as InboxItem),
    ).toBeNull();
  });

  it("parses a Linear issue URL without an API call", () => {
    expect(
      parseLinearWorkItemUrl(
        "See https://linear.app/acme/issue/sw-29/fix-the-thing please",
      ),
    ).toEqual({
      kind: "linear",
      identifier: "SW-29",
      repo: "SW",
      number: 29,
      url: "https://linear.app/acme/issue/SW-29",
    });
    expect(
      parseLinearWorkItemUrl("https://linear.app/acme/project/x"),
    ).toBeNull();
  });

  it("resolves a ticket key against Linear before trusting the title model", async () => {
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === "linear_status") return { connected: true };
      if (command === "linear_list_teams") {
        return [{ id: "team-1", key: "SW", name: "Software" }];
      }
      if (command === "linear_issue_lookup") {
        expect(args).toEqual({ key: "SW-29" });
        return {
          id: "issue-uuid",
          identifier: "SW-29",
          number: 29,
          repo: "SW",
          url: "https://linear.app/acme/issue/SW-29",
        };
      }
      throw new Error(`Unexpected command: ${String(command)}`);
    });

    await expect(
      resolveLinkedWorkItem("/dev-implement SW-29", "/tmp/codex", {
        kind: "issue",
        number: 29,
      }),
    ).resolves.toEqual({
      kind: "linear",
      identifier: "SW-29",
      id: "issue-uuid",
      repo: "SW",
      number: 29,
      url: "https://linear.app/acme/issue/SW-29",
    });
    expect(invoke).not.toHaveBeenCalledWith(
      "git_github_repo",
      expect.anything(),
    );
  });

  it("does not turn a ticket key into a GitHub issue when Linear cannot resolve it", async () => {
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "linear_status") return { connected: true };
      if (command === "linear_list_teams") throw new Error("offline");
      if (command === "linear_issue_lookup") throw new Error("not found");
      if (command === "git_github_repo") return "openai/codex";
      throw new Error(`Unexpected command: ${String(command)}`);
    });

    await expect(
      resolveLinkedWorkItem("/dev-implement SW-29", "/tmp/codex", {
        kind: "issue",
        number: 29,
      }),
    ).resolves.toBeNull();
  });

  it("skips Linear lookups entirely when Linear is not connected", async () => {
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "linear_status") return { connected: false };
      if (command === "git_github_repo") return "openai/codex";
      throw new Error(`Unexpected command: ${String(command)}`);
    });

    await expect(
      resolveLinkedWorkItem("Encode as UTF-8 for issue #7", "/tmp/codex", null),
    ).resolves.toEqual({
      kind: "issue",
      repo: "openai/codex",
      number: 7,
      url: "https://github.com/openai/codex/issues/7",
    });
    expect(invoke).not.toHaveBeenCalledWith(
      "linear_issue_lookup",
      expect.anything(),
    );
  });

  it("only looks up ticket keys whose team Linear knows", async () => {
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === "linear_status") return { connected: true };
      if (command === "linear_list_teams") {
        return [{ id: "team-1", key: "sw", name: "Software" }];
      }
      if (command === "linear_issue_lookup") {
        expect(args).toEqual({ key: "SW-29" });
        return {
          id: "issue-uuid",
          identifier: "SW-29",
          number: 29,
          repo: "SW",
          url: "https://linear.app/acme/issue/SW-29",
        };
      }
      throw new Error(`Unexpected command: ${String(command)}`);
    });

    const linked = await resolveLinkedWorkItem(
      "Hash with SHA-256 then do SW-29",
      "/tmp/codex",
      null,
    );
    expect(linked?.kind).toBe("linear");
    expect(invoke).not.toHaveBeenCalledWith("linear_issue_lookup", {
      key: "SHA-256",
    });
  });

  it("keeps every ticket key when the team list is unavailable", () => {
    expect(ticketKeysForTeams(["SHA-256", "SW-29"], null)).toEqual([
      "SHA-256",
      "SW-29",
    ]);
    expect(ticketKeysForTeams(["SHA-256", "SW-29"], new Set(["SW"]))).toEqual([
      "SW-29",
    ]);
  });

  it("lists distinct ticket keys in order of appearance", () => {
    expect(
      ticketKeysInMessage("Fix sw-29 and ENG-4, then SW-29 again, not SW-007"),
    ).toEqual(["SW-29", "ENG-4"]);
  });

  it("keeps the current PR link when the message also names a ticket", async () => {
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "git_pr_status") {
        return { number: 2022, url: "https://github.com/acme/app/pull/2022" };
      }
      throw new Error(`Unexpected command: ${String(command)}`);
    });

    await expect(
      resolveLinkedWorkItem("Review this PR against SW-29", "/tmp/app", null),
    ).resolves.toEqual({
      kind: "pr",
      repo: "acme/app",
      number: 2022,
      url: "https://github.com/acme/app/pull/2022",
    });
    expect(invoke).not.toHaveBeenCalledWith("linear_status");
  });

  it("keeps the model hint when the only ticket-shaped text is not a known team", async () => {
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "linear_status") return { connected: true };
      if (command === "linear_list_teams") {
        return [{ id: "team-1", key: "SW", name: "Software" }];
      }
      if (command === "git_github_repo") return "acme/app";
      throw new Error(`Unexpected command: ${String(command)}`);
    });

    await expect(
      resolveLinkedWorkItem("Fix #29 UTF-8 handling", "/tmp/app", {
        kind: "issue",
        number: 29,
      }),
    ).resolves.toEqual({
      kind: "issue",
      repo: "acme/app",
      number: 29,
      url: "https://github.com/acme/app/issues/29",
    });
    expect(invoke).not.toHaveBeenCalledWith(
      "linear_issue_lookup",
      expect.anything(),
    );
  });

  it("drops only a model hint that repeats a plausible ticket number", async () => {
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "linear_status") return { connected: true };
      if (command === "linear_list_teams") throw new Error("offline");
      if (command === "linear_issue_lookup") throw new Error("not found");
      if (command === "git_github_repo") return "acme/app";
      throw new Error(`Unexpected command: ${String(command)}`);
    });

    await expect(
      resolveLinkedWorkItem(
        "Fix GitHub #42: handle UTF-8 decoding",
        "/tmp/app",
        {
          kind: "issue",
          number: 42,
        },
      ),
    ).resolves.toMatchObject({ kind: "issue", number: 42 });
    await expect(
      resolveLinkedWorkItem("Handle UTF-8 decoding", "/tmp/app", {
        kind: "issue",
        number: 8,
      }),
    ).resolves.toBeNull();
  });

  it("does not match a Linear row whose UUID differs even when identifiers repeat", () => {
    const linked = {
      kind: "linear" as const,
      identifier: "SW-29",
      id: "workspace-a-uuid",
      repo: "SW",
      number: 29,
      url: "https://linear.app/a/issue/SW-29",
    };
    const sameIdentifier = {
      provider: "linear",
      kind: "linear",
      id: "workspace-b-uuid",
      identifier: "SW-29",
      number: 29,
      repo: "SW",
    } as InboxItem;
    expect(inboxItemMatchesLinkedWorkItem(sameIdentifier, linked)).toBe(false);
    expect(
      inboxItemMatchesLinkedWorkItem(
        { ...sameIdentifier, id: undefined },
        linked,
      ),
    ).toBe(true);
  });

  it("rejects a Linear link whose URL is not an issue page", () => {
    expect(
      normalizeLinkedWorkItem({
        kind: "linear",
        identifier: "SW-29",
        url: "https://linear.app/settings",
      }),
    ).toBeUndefined();
    expect(
      normalizeLinkedWorkItem({
        kind: "linear",
        identifier: "SW-29",
        url: "https://linear.app/acme/project/roadmap",
      }),
    ).toBeUndefined();
  });

  it("keeps a GitHub hint whose number also stands alone when Linear lookups fail", async () => {
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "linear_status") return { connected: true };
      if (command === "linear_list_teams") throw new Error("offline");
      if (command === "linear_issue_lookup") throw new Error("not found");
      if (command === "git_github_repo") return "acme/app";
      throw new Error(`Unexpected command: ${String(command)}`);
    });

    await expect(
      resolveLinkedWorkItem(
        "Fix GitHub #8: handle UTF-8 decoding",
        "/tmp/app",
        {
          kind: "issue",
          number: 8,
        },
      ),
    ).resolves.toEqual({
      kind: "issue",
      repo: "acme/app",
      number: 8,
      url: "https://github.com/acme/app/issues/8",
    });
  });

  it("rejects a persisted Linear link whose identifier disagrees with its URL", () => {
    expect(
      normalizeLinkedWorkItem({
        kind: "linear",
        identifier: "SW-29",
        url: "https://linear.app/acme/issue/ENG-1",
      }),
    ).toBeUndefined();
    expect(
      normalizeLinkedWorkItem({
        kind: "linear",
        identifier: "SW-29",
        url: "https://linear.app/acme/issue/sw-29/some-title",
      }),
    ).toMatchObject({ identifier: "SW-29" });
  });

  it("does not match an Inbox row from another workspace when no UUID is stored", () => {
    const linked = {
      kind: "linear" as const,
      identifier: "SW-29",
      repo: "SW",
      number: 29,
      url: "https://linear.app/workspace-a/issue/SW-29",
    };
    const row = {
      provider: "linear",
      kind: "linear",
      id: "uuid-b",
      identifier: "SW-29",
      number: 29,
      repo: "SW",
      url: "https://linear.app/workspace-b/issue/SW-29",
    } as InboxItem;
    expect(inboxItemMatchesLinkedWorkItem(row, linked)).toBe(false);
    expect(
      inboxItemMatchesLinkedWorkItem(
        { ...row, url: "https://linear.app/workspace-a/issue/SW-29" },
        linked,
      ),
    ).toBe(true);
    expect(
      inboxItemMatchesLinkedWorkItem(
        { ...row, url: "https://linear.app/issue/SW-29" },
        linked,
      ),
    ).toBe(true);
  });

  it("reads the workspace from a Linear URL", () => {
    expect(
      linearWorkspaceFromUrl("https://linear.app/Acme/issue/SW-29/slug"),
    ).toBe("acme");
    expect(linearWorkspaceFromUrl("https://linear.app/issue/SW-29")).toBeNull();
    expect(linearWorkspaceFromUrl("https://example.com")).toBeNull();
  });

  it("parses an org-less Linear URL and rejects leading zeros", () => {
    expect(parseLinearWorkItemUrl("https://linear.app/issue/LIN-123")).toEqual({
      kind: "linear",
      identifier: "LIN-123",
      repo: "LIN",
      number: 123,
      url: "https://linear.app/issue/LIN-123",
    });
    expect(
      parseLinearWorkItemUrl("https://linear.app/acme/issue/SW-00029"),
    ).toBeNull();
  });

  it("normalizes persisted links of both kinds and rejects the rest", () => {
    expect(
      normalizeLinkedWorkItem({
        kind: "pr",
        repo: "acme/app",
        number: 7,
        url: "https://example.com",
      }),
    ).toEqual({
      kind: "pr",
      repo: "acme/app",
      number: 7,
      url: "https://github.com/acme/app/pull/7",
    });
    expect(
      normalizeLinkedWorkItem({
        kind: "linear",
        identifier: "sw-29",
        url: "https://linear.app/acme/issue/SW-29",
      }),
    ).toEqual({
      kind: "linear",
      identifier: "SW-29",
      repo: "SW",
      number: 29,
      url: "https://linear.app/acme/issue/SW-29",
    });
    expect(
      normalizeLinkedWorkItem({
        kind: "linear",
        identifier: "SW-29",
        url: "https://evil.example/SW-29",
      }),
    ).toBeUndefined();
    expect(
      normalizeLinkedWorkItem({ kind: "jira", repo: "x/y", number: 1 }),
    ).toBeUndefined();
    expect(normalizeLinkedWorkItem("SW-29")).toBeUndefined();
  });

  it("finds sessions related to the same GitHub Inbox item", () => {
    const item = {
      provider: "github",
      kind: "pr",
      repo: "Acme/App",
      number: 42,
    } as InboxItem;
    const matching = {
      id: "matching",
      linkedWorkItem: {
        kind: "pr" as const,
        repo: "acme/app",
        number: 42,
        url: "https://github.com/acme/app/pull/42",
      },
    };
    const sessions = [
      matching,
      {
        id: "other-number",
        linkedWorkItem: { ...matching.linkedWorkItem, number: 43 },
      },
      {
        id: "other-kind",
        linkedWorkItem: {
          ...matching.linkedWorkItem,
          kind: "issue" as const,
        },
      },
      { id: "unlinked" },
    ];

    expect(relatedSessionsForInboxItem(item, sessions)).toEqual([matching]);
    expect(
      relatedSessionsForInboxItem(
        { ...item, provider: "linear", kind: "linear" } as InboxItem,
        sessions,
      ),
    ).toEqual([]);
  });

  it("finds sessions related to the same Linear Inbox item", () => {
    const item = {
      provider: "linear",
      kind: "linear",
      id: "issue-uuid",
      identifier: "sw-29",
      number: 29,
      repo: "SW",
    } as InboxItem;
    const matching = {
      id: "matching",
      linkedWorkItem: {
        kind: "linear" as const,
        identifier: "SW-29",
        repo: "SW",
        number: 29,
        url: "https://linear.app/acme/issue/SW-29",
      },
    };
    const sessions = [
      matching,
      {
        id: "other-team",
        linkedWorkItem: {
          ...matching.linkedWorkItem,
          identifier: "ENG-29",
          repo: "ENG",
        },
      },
      {
        id: "github",
        linkedWorkItem: {
          kind: "issue" as const,
          repo: "acme/app",
          number: 29,
          url: "https://github.com/acme/app/issues/29",
        },
      },
    ];

    expect(relatedSessionsForInboxItem(item, sessions)).toEqual([matching]);
  });
});
