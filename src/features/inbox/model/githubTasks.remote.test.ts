import { beforeEach, expect, it, vi } from "vitest";

const { local, request, machineFor } = vi.hoisted(() => ({
  local: vi.fn(),
  request: vi.fn(),
  machineFor: vi.fn(async (environmentId: string) => ({ id: environmentId })),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: local }));
vi.mock("../../connections/model/connections", () => ({
  remoteRequest: request,
  remoteMachineFor: machineFor,
}));
import "../../connections/model/remoteCommands";
import {
  clearInboxCache,
  githubRepositories,
  githubWorkItem,
  githubWorkItemDetails,
  githubWorkItemThread,
  githubPrDiff,
  githubWorkItemComment,
  githubPrAction,
  githubRepo,
  githubStatus,
  listGithubWorkItems,
  listInboxItems,
} from "./githubTasks";
import { fetchGithubCheckDetails, fetchGithubPrChecks } from "./githubPrChecks";

const cwd = "remote://ssh-host/home/remote/my repo";
const repo = "acme/web";
const query = {
  kind: "issue" as const,
  assignedToMe: true,
  state: "open" as const,
  search: "",
};

beforeEach(() => {
  clearInboxCache();
  local.mockReset().mockResolvedValue({ connected: false });
  request.mockReset().mockResolvedValue([]);
  machineFor.mockClear();
});

it.each([
  ["git_github_repo", () => githubRepo(cwd), repo],
  ["git_github_repositories", () => githubRepositories(cwd), [repo]],
  ["git_github_work_items", () => listGithubWorkItems(cwd, repo, query), []],
  ["git_github_work_item", () => githubWorkItem(cwd, repo, "issue", 1), {}],
  [
    "git_github_work_item_details",
    () => githubWorkItemDetails(cwd, repo, "issue", 1),
    {},
  ],
  [
    "git_github_work_item_thread",
    () => githubWorkItemThread(cwd, repo, "issue", 1),
    {},
  ],
  ["git_github_pr_diff", () => githubPrDiff(cwd, repo, 1), {}],
  ["git_github_pr_checks", () => fetchGithubPrChecks(cwd, repo, 1), {}],
  [
    "git_github_check_details",
    () => fetchGithubCheckDetails(cwd, repo, "12"),
    {},
  ],
  [
    "git_github_work_item_comment",
    () => githubWorkItemComment(cwd, repo, "issue", 1, "hello"),
    "https://github.com/comment",
  ],
  ["git_github_pr_action", () => githubPrAction(cwd, repo, 1, "close"), {}],
] as const)(
  "routes %s to the owning host with a filesystem cwd",
  async (command, run, response) => {
    request.mockResolvedValue(response);
    expect(await run()).toEqual(response);
    expect(machineFor).toHaveBeenCalledWith("ssh-host");
    expect(request).toHaveBeenCalledWith("ssh-host", "workspace.run", {
      command,
      args: expect.objectContaining({ cwd: "/home/remote/my repo" }),
    });
    expect(local).not.toHaveBeenCalled();
  },
);

it("keeps desktop status and local Inbox calls on Tauri", async () => {
  local
    .mockResolvedValueOnce({ installed: true })
    .mockResolvedValueOnce([repo]);
  await githubStatus();
  await githubRepositories("/Users/local/web");
  expect(local).toHaveBeenNthCalledWith(1, "git_github_status");
  expect(local).toHaveBeenNthCalledWith(2, "git_github_repositories", {
    cwd: "/Users/local/web",
  });
  expect(request).not.toHaveBeenCalled();
});

it("uses both accounts for the same repo and keeps the preferred remote project", async () => {
  const item = {
    kind: "issue",
    number: 1,
    repo,
    title: "Assigned remotely",
    updatedAt: "2026-10-10",
    labels: [],
    assignees: [],
  };
  request.mockImplementation(async (_id, _method, { command, args }) =>
    command === "git_github_repositories"
      ? [repo]
      : args.kind === "issue"
        ? [item]
        : [],
  );
  local.mockImplementation(async (command) =>
    command === "git_github_repositories"
      ? [repo]
      : command === "git_github_work_items"
        ? []
        : { connected: false },
  );
  const projects = [{ path: "/Users/local/web" }, { path: cwd }];
  const result = await listInboxItems(projects, query);
  expect(result.items).toEqual([
    expect.objectContaining({ title: "Assigned remotely", projectPath: cwd }),
  ]);
  expect(
    request.mock.calls.filter(
      (call) => call[2].command === "git_github_work_items",
    ),
  ).toHaveLength(2);
  expect(
    local.mock.calls.filter((call) => call[0] === "git_github_work_items"),
  ).toHaveLength(2);
  await listInboxItems(projects, query);
  expect(request).toHaveBeenCalledTimes(3); // Preserve the two-minute list throttle.
});

it("does not reuse a desktop item, details, thread or diff on a remote account", async () => {
  for (const run of [
    githubWorkItem,
    githubWorkItemDetails,
    githubWorkItemThread,
  ]) {
    local.mockResolvedValue({ source: "desktop" });
    request.mockResolvedValue({ source: "remote" });
    await run("/Users/local/web", repo, "pr", 1, { maxAgeMs: 30_000 });
    expect(await run(cwd, repo, "pr", 1, { maxAgeMs: 30_000 })).toEqual({
      source: "remote",
    });
  }
  await githubPrDiff("/Users/local/web", repo, 1, { maxAgeMs: 30_000 });
  expect(await githubPrDiff(cwd, repo, 1, { maxAgeMs: 30_000 })).toEqual({
    source: "remote",
  });
});

it("reports an outdated host or real host failure without falling back to desktop gh", async () => {
  request.mockRejectedValueOnce(new Error("Unsupported workspace command"));
  await expect(githubRepositories(cwd)).rejects.toThrow("Update MonoCode Host");
  request.mockRejectedValueOnce(new Error("remote gh authentication failed"));
  await expect(githubRepositories(cwd)).rejects.toThrow(
    "remote gh authentication failed",
  );
  expect(local).not.toHaveBeenCalled();
});
