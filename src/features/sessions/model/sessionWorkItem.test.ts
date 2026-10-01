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
  linkedWorkItemInboxKey,
  linkedWorkItemFromAutomationEvent,
  linkedWorkItemFromInboxItem,
  parseGithubWorkItemUrl,
  parseLinearWorkItemUrl,
  relatedSessionsForInboxItem,
  resolveLinkedWorkItem,
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

  it("lists distinct ticket keys in order of appearance", () => {
    expect(
      ticketKeysInMessage("Fix SW-29 and ENG-4, then SW-29 again"),
    ).toEqual(["SW-29", "ENG-4"]);
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
