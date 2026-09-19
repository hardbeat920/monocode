import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  clearInboxCache,
  githubPrDiff,
  githubRepositories,
  githubWorkItemComment,
  githubWorkItemDetails,
  githubWorkItemThread,
  listInboxItems,
  peekGithubWorkItemDetails,
  type GithubWorkItem,
} from "./githubTasks";

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
      if (command === "linear_status" || command === "gitlab_status") {
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
      if (command === "linear_status" || command === "gitlab_status") {
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
      kind: "pr",
      number: 10,
    });
    expect(invoke).toHaveBeenCalledWith("git_github_pr_diff", {
      cwd: "/tmp/web",
      repo: "acme/web",
      number: 10,
      fullContext: false,
    });
    expect(invoke).toHaveBeenCalledWith("git_github_work_item_comment", {
      cwd: "/tmp/web",
      repo: "acme/web",
      kind: "issue",
      number: 10,
      body: "Looks good",
      inReplyTo: "",
    });
  });
});
