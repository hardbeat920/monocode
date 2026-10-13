import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  clearInboxCache,
  githubPrAction,
  githubPrDiff,
  githubRepositories,
  githubWorkItemComment,
  githubWorkItemDetails,
  githubWorkItemThread,
  inboxIdentityKey,
  listInboxItems,
  peekGithubWorkItemDetails,
  type GithubWorkItem,
} from "./githubTasks";
import { githubHostOf, setGithubHost } from "./githubHost";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

beforeEach(() => {
  clearInboxCache();
  vi.mocked(invoke).mockReset();
});

function workItem(repo: string, kind: "issue" | "pr"): GithubWorkItem {
  return {
    kind,
    number: 10,
    title: `${repo} item`,
    url: `https://github.com/${repo}/${kind === "pr" ? "pull" : "issues"}/10`,
    state: "open",
    updatedAt: "2026-09-16T08:00:00Z",
    labels: [],
    assignees: [],
    draft: false,
    repo,
  };
}

describe("GitHub fork repositories", () => {
  it("caches the local repository and parent metadata", async () => {
    vi.mocked(invoke).mockResolvedValue(["maya/web", "acme/web"] as never);

    await expect(githubRepositories("/tmp/web")).resolves.toEqual([
      "maya/web",
      "acme/web",
    ]);
    await expect(githubRepositories("/tmp/web/")).resolves.toEqual([
      "maya/web",
      "acme/web",
    ]);

    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith("git_github_repositories", {
      cwd: "/tmp/web",
    });
  });

  it("fetches a shared parent once and keeps the preferred local checkout", async () => {
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      const input = args as Record<string, unknown> | undefined;
      if (command === "git_github_repositories") {
        return (
          input?.cwd === "/tmp/fork-a"
            ? ["maya/web", "acme/web"]
            : ["lin/web", "ACME/web"]
        ) as never;
      }
      if (command === "git_github_work_items") {
        const repo = String(input?.repo ?? "");
        const kind = input?.kind as "issue" | "pr";
        return (
          repo.toLowerCase() === "acme/web" && kind === "issue"
            ? [workItem("acme/web", kind)]
            : []
        ) as never;
      }
      if (
        command === "linear_status" ||
        command === "jira_status" ||
        command === "gitlab_status" ||
        command === "azure_devops_status"
      ) {
        return { connected: false } as never;
      }
      throw new Error(`Unexpected command: ${command}`);
    });

    const result = await listInboxItems(
      [{ path: "/tmp/fork-a" }, { path: "/tmp/fork-b" }],
      { assignedToMe: false, state: "open", search: "" },
    );

    expect(result.items).toHaveLength(1);
    expect(result.items[0]).toMatchObject({
      repo: "acme/web",
      projectPath: "/tmp/fork-a",
    });
    const listCalls = vi
      .mocked(invoke)
      .mock.calls.filter(([command]) => command === "git_github_work_items");
    expect(listCalls).toHaveLength(6);
    expect(
      listCalls.filter(
        ([, args]) =>
          String((args as Record<string, unknown>).repo).toLowerCase() ===
          "acme/web",
      ),
    ).toHaveLength(2);
  });

  it("reports an error when repository discovery fails", async () => {
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "git_github_repositories") {
        throw new Error("not a GitHub repository");
      }
      if (
        command === "linear_status" ||
        command === "jira_status" ||
        command === "gitlab_status" ||
        command === "azure_devops_status"
      ) {
        return { connected: false } as never;
      }
      throw new Error(`Unexpected command: ${command}`);
    });

    await expect(
      listInboxItems([{ path: "/tmp/local" }], {
        assignedToMe: false,
        state: "open",
        search: "",
      }),
    ).resolves.toEqual({
      items: [],
      errors: { github: "not a GitHub repository" },
    });
  });

  it("preserves the last list while GitHub is rate limited and refreshes it on recovery", async () => {
    let limited = false;
    const projects = [{ path: "/tmp/web" }];
    const query = { assignedToMe: false, state: "open", search: "" } as const;
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === "git_github_repositories") return ["acme/web"];
      if (command === "git_github_work_items") {
        if (limited)
          throw new Error("GraphQL: API rate limit already exceeded");
        return [workItem("acme/web", (args as { kind: "issue" | "pr" }).kind)];
      }
      return { connected: false };
    });
    const initial = await listInboxItems(projects, query);
    limited = true;
    const stale = await listInboxItems(projects, query, { force: true });
    expect(stale.items).toEqual(initial.items);
    expect(stale.errors.github).toContain("rate limit");
    limited = false;
    const recovered = await listInboxItems(projects, query, { force: true });
    expect(recovered.items).toEqual(initial.items);
    expect(recovered.errors).toEqual({});
  });
});

describe("repository-qualified GitHub item operations", () => {
  it("passes the repository through and isolates same-number caches", async () => {
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      const input = args as Record<string, unknown>;
      if (command === "git_github_work_item_details") {
        return { body: String(input.repo), author: "octocat" } as never;
      }
      if (command === "git_github_work_item_thread") {
        return {
          comments: [],
          commits: [],
          truncated: false,
          reviewDecision: "",
          baseRefName: "main",
          headRefName: "feature",
        } as never;
      }
      if (command === "git_github_pr_diff") {
        return {
          additions: 1,
          deletions: 0,
          files: [],
          patch: "diff",
          truncated: false,
        } as never;
      }
      if (command === "git_github_work_item_comment") {
        return "https://github.com/acme/web/issues/10#issuecomment-1" as never;
      }
      throw new Error(`Unexpected command: ${command}`);
    });

    await githubWorkItemDetails("/tmp/web", "maya/web", "issue", 10);
    await githubWorkItemDetails("/tmp/web", "acme/web", "issue", 10);
    expect(peekGithubWorkItemDetails("maya/web", "issue", 10)?.body).toBe(
      "maya/web",
    );
    expect(peekGithubWorkItemDetails("acme/web", "issue", 10)?.body).toBe(
      "acme/web",
    );

    await githubWorkItemThread("/tmp/web", "acme/web", "pr", 10);
    await githubPrDiff("/tmp/web", "acme/web", 10);
    await githubWorkItemComment(
      "/tmp/web",
      "acme/web",
      "issue",
      10,
      "Looks good",
    );

    expect(invoke).toHaveBeenCalledWith("git_github_work_item_thread", {
      cwd: "/tmp/web",
      repo: "acme/web",
      host: "github.com",
      kind: "pr",
      number: 10,
    });
    expect(invoke).toHaveBeenCalledWith("git_github_pr_diff", {
      cwd: "/tmp/web",
      repo: "acme/web",
      host: "github.com",
      number: 10,
      fullContext: false,
    });
    expect(invoke).toHaveBeenCalledWith("git_github_work_item_comment", {
      cwd: "/tmp/web",
      repo: "acme/web",
      host: "github.com",
      kind: "issue",
      number: 10,
      body: "Looks good",
      inReplyTo: "",
    });
  });
});

describe("GitHub items on different servers", () => {
  const ghes = "github.example.com";

  it("acts on the item's own server even after the configured host changes", async () => {
    setGithubHost(ghes);
    try {
      vi.mocked(invoke).mockResolvedValue({
        ...workItem("acme/web", "pr"),
        state: "merged",
      } as never);
      // A github.com item opened in another window must merge on github.com.
      await githubPrAction("/tmp/web", "acme/web", 10, "squash", {
        host: githubHostOf("https://github.com/acme/web/pull/10"),
      });
      expect(invoke).toHaveBeenCalledWith(
        "git_github_pr_action",
        expect.objectContaining({ repo: "acme/web", host: "github.com" }),
      );
      vi.mocked(invoke).mockResolvedValue(
        "https://x/1#issuecomment-1" as never,
      );
      await githubWorkItemComment("/tmp/web", "acme/web", "pr", 10, "hi");
      expect(invoke).toHaveBeenCalledWith(
        "git_github_work_item_comment",
        expect.objectContaining({ host: ghes }),
      );
    } finally {
      setGithubHost(null);
    }
  });

  it("keeps same-slug items on different servers apart", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ body: "dotcom" } as never);
    await githubWorkItemDetails("/tmp/web", "acme/web", "pr", 10, {
      host: "github.com",
    });
    expect(peekGithubWorkItemDetails("acme/web", "pr", 10, ghes)).toBeNull();
    expect(
      peekGithubWorkItemDetails("acme/web", "pr", 10, "github.com")?.body,
    ).toBe("dotcom");
    const dotcom = {
      provider: "github" as const,
      kind: "pr" as const,
      repo: "acme/web",
      number: 10,
      url: "https://github.com/acme/web/pull/10",
    };
    const enterprise = { ...dotcom, url: `https://${ghes}/acme/web/pull/10` };
    // github.com keys stay unprefixed so seen state saved earlier still matches.
    expect(inboxIdentityKey(dotcom)).toBe("acme/web:pr:10");
    expect(inboxIdentityKey(enterprise)).toBe(`${ghes}/acme/web:pr:10`);
  });
});
