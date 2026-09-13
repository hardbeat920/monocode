import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  clearInboxCache,
  githubWorkItem,
  inboxItemKey,
  type GithubWorkItem,
  type InboxItem,
} from "./githubTasks";
import {
  inboxItemMatchesLinkedWorkItem,
  linkedWorkItemInboxKey,
  linkedWorkItemFromInboxItem,
  parseGithubWorkItemUrl,
  relatedSessionsForInboxItem,
  resolveLinkedWorkItem,
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

  it("does not associate a Linear Inbox item", () => {
    expect(
      linkedWorkItemFromInboxItem({
        provider: "linear",
        kind: "linear",
        number: 12,
        repo: "",
      } as InboxItem),
    ).toBeNull();
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
});
