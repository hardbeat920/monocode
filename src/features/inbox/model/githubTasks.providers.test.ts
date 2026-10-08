import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  clearInboxCache,
  listInboxItems,
  type GithubTaskKind,
  type GithubWorkItem,
} from "./githubTasks";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

function workItem(repo: string, kind: GithubTaskKind) {
  return {
    kind,
    number: 10,
    title: `${kind} item`,
    url: `https://example.com/${repo}/${kind}/10`,
    state: "open",
    updatedAt: "2026-09-16T08:00:00Z",
    labels: [{ name: "bug", color: "ff0000" }],
    assignees: [{ login: "maya", avatarUrl: "https://example.com/maya.png" }],
    draft: kind === "pr",
    repo,
    attentionReason: "assigned",
  } satisfies GithubWorkItem & { attentionReason: string };
}

describe.each([
  { provider: "gitlab", commandPrefix: "gitlab" },
  { provider: "azuredevops", commandPrefix: "azure_devops" },
] as const)("$provider inbox fetching", ({ provider, commandPrefix }) => {
  const list = vi.fn(async ({ kind }: { kind: GithubTaskKind }) => [
    workItem("", kind),
  ]);
  let connected: boolean;
  const projects = [
    { path: "/tmp/first/" },
    { path: "/tmp/first" },
    { path: "/tmp/second" },
    { path: "/tmp/missing" },
    { path: "/tmp/blank" },
  ];
  const query = { assignedToMe: false, state: "open", search: "" } as const;

  beforeEach(() => {
    clearInboxCache();
    connected = true;
    list.mockReset();
    list.mockImplementation(async ({ kind }) => [workItem("", kind)]);
    vi.mocked(invoke).mockReset();
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === "git_github_repositories") return ["github/repo"];
      if (command === "git_github_work_items") return [];
      if (command === `${commandPrefix}_status`) return { connected };
      if (
        command === "linear_status" ||
        command === "jira_status" ||
        command === "gitlab_status" ||
        command === "azure_devops_status" ||
        command === "bitbucket_status"
      ) {
        return { connected: false };
      }
      if (command === `${commandPrefix}_repo`) {
        const { cwd } = args as { cwd: string };
        if (cwd === "/tmp/first") return " acme/web ";
        if (cwd === "/tmp/second") return "ACME/WEB";
        if (cwd === "/tmp/blank") return "  ";
        throw new Error("not a provider repository");
      }
      if (
        command === `${commandPrefix}_list_work_items` ||
        command === `${commandPrefix}_list_todos`
      ) {
        return list(args as { kind: GithubTaskKind });
      }
      throw new Error(`Unexpected command: ${command}`);
    });
  });

  it.each(["open", "all"] as const)(
    "fetches %s items once per repository using the first checkout",
    async (state) => {
      const result = await listInboxItems(projects, { ...query, state });

      expect(result).toEqual({
        items: ["issue", "pr"].map((kind) => ({
          ...workItem("", kind as GithubTaskKind),
          repo: "acme/web",
          projectPath: "/tmp/first",
          provider,
        })),
        errors: {},
      });
      expect(list).toHaveBeenCalledTimes(2);
      for (const kind of ["issue", "pr"]) {
        expect(invoke).toHaveBeenCalledWith(
          `${commandPrefix}_list_work_items`,
          {
            cwd: "/tmp/first",
            kind,
            assignedToMe: false,
            state,
            limit: state === "all" ? 100 : undefined,
          },
        );
      }
      expect(invoke).not.toHaveBeenCalledWith(
        `${commandPrefix}_list_todos`,
        expect.anything(),
      );
      expect(
        vi
          .mocked(invoke)
          .mock.calls.filter(([cmd]) => cmd === `${commandPrefix}_repo`),
      ).toHaveLength(4);
    },
  );

  it.each(["open", "all"] as const)(
    "fetches %s assigned items globally and matches local checkouts ignoring case",
    async (state) => {
      list.mockImplementation(async ({ kind }) => [
        workItem(kind === "issue" ? "ACME/WEB" : "other/remote", kind),
      ]);

      const result = await listInboxItems(projects, {
        ...query,
        assignedToMe: true,
        state,
      });

      expect(result).toEqual({
        items: [
          { ...workItem("other/remote", "pr"), provider, projectPath: "" },
          {
            ...workItem("ACME/WEB", "issue"),
            provider,
            projectPath: "/tmp/first",
          },
        ],
        errors: {},
      });
      expect(list).toHaveBeenCalledTimes(2);
      for (const kind of ["issue", "pr"]) {
        expect(invoke).toHaveBeenCalledWith(`${commandPrefix}_list_todos`, {
          kind,
          limit: state === "all" ? 100 : undefined,
        });
      }
      expect(invoke).not.toHaveBeenCalledWith(
        `${commandPrefix}_list_work_items`,
        expect.anything(),
      );
    },
  );

  it("preserves the repository supplied by a work item", async () => {
    list.mockImplementation(async ({ kind }) => [
      workItem("canonical/repo", kind),
    ]);

    const result = await listInboxItems(projects, query);

    expect(result.items.map((item) => item.repo)).toEqual([
      "canonical/repo",
      "canonical/repo",
    ]);
  });

  it.each([false, true])(
    "keeps a successful batch when the other kind fails (assigned: %s)",
    async (assignedToMe) => {
      list.mockImplementation(async ({ kind }) => {
        if (kind === "issue") throw new Error("issues unavailable");
        return [workItem("acme/web", kind)];
      });

      await expect(
        listInboxItems(projects, { ...query, assignedToMe }),
      ).resolves.toEqual({
        items: [
          {
            ...workItem("acme/web", "pr"),
            provider,
            projectPath: "/tmp/first",
          },
        ],
        errors: {},
      });
    },
  );

  it.each([false, true])(
    "reports the first error when every fetch fails (assigned: %s)",
    async (assignedToMe) => {
      list.mockImplementation(async ({ kind }) => {
        throw new Error(`${kind} unavailable`);
      });

      await expect(
        listInboxItems(projects, { ...query, assignedToMe }),
      ).resolves.toEqual({
        items: [],
        errors: { [provider]: "issue unavailable" },
      });
    },
  );

  it("returns an empty inbox when no local repositories resolve", async () => {
    await expect(
      listInboxItems([{ path: "/tmp/missing" }], query),
    ).resolves.toEqual({
      items: [],
      errors: {},
    });
    expect(list).not.toHaveBeenCalled();
  });

  it("fetches assigned items even without local projects", async () => {
    list.mockImplementation(async ({ kind }) => [
      workItem("other/remote", kind),
    ]);

    const result = await listInboxItems([], { ...query, assignedToMe: true });

    expect(result.items).toEqual(
      ["issue", "pr"].map((kind) => ({
        ...workItem("other/remote", kind as GithubTaskKind),
        provider,
        projectPath: "",
      })),
    );
    expect(result.errors).toEqual({});
    expect(list).toHaveBeenCalledTimes(2);
  });

  it("skips disconnected providers", async () => {
    connected = false;

    await expect(listInboxItems(projects, query)).resolves.toEqual({
      items: [],
      errors: {},
    });
    expect(list).not.toHaveBeenCalled();
    expect(invoke).not.toHaveBeenCalledWith(
      `${commandPrefix}_repo`,
      expect.anything(),
    );
  });
});

describe("bitbucket inbox fetching", () => {
  const projects = [{ path: "/tmp/first" }, { path: "/tmp/other" }];
  const query = { assignedToMe: false, state: "open", search: "" } as const;
  let calls: { command: string; args: unknown }[];

  beforeEach(() => {
    clearInboxCache();
    calls = [];
    vi.mocked(invoke).mockReset();
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      calls.push({ command, args });
      if (command === "git_github_repositories") return ["github/repo"];
      if (command === "git_github_work_items") return [];
      if (command === "bitbucket_status") return { connected: true };
      if (command.endsWith("_status")) return { connected: false };
      if (command === "bitbucket_repo") {
        const { cwd } = args as { cwd: string };
        if (cwd === "/tmp/first") return "acme/web";
        throw new Error("no bitbucket remote");
      }
      if (command === "bitbucket_list_work_items") {
        const { kind } = args as { kind: GithubTaskKind };
        return kind === "pr" ? [{ ...workItem("acme/web", "pr") }] : [];
      }
      throw new Error(`Unexpected command: ${command}`);
    });
  });

  it("lists per repository and skips checkouts without a Bitbucket remote", async () => {
    const result = await listInboxItems(projects, query);

    expect(result.errors).toEqual({});
    // Bitbucket retired issues, so only pull requests are requested.
    expect(
      calls
        .filter((call) => call.command === "bitbucket_list_work_items")
        .map((call) => (call.args as { kind: string }).kind),
    ).toEqual(["pr"]);
    expect(result.items).toHaveLength(1);
    expect(result.items[0]).toMatchObject({
      provider: "bitbucket",
      kind: "pr",
      repo: "acme/web",
      projectPath: "/tmp/first",
    });
  });

  it("answers needs-attention per repository because there is no to-do feed", async () => {
    await listInboxItems(projects, { ...query, assignedToMe: true });

    const listed = calls.filter(
      (call) => call.command === "bitbucket_list_work_items",
    );
    expect(
      listed.map((call) => (call.args as { kind: string }).kind).sort(),
    ).toEqual(["pr"]);
    for (const call of listed) {
      expect(call.args).toMatchObject({
        cwd: "/tmp/first",
        assignedToMe: true,
      });
    }
  });

  it("reports a provider error without dropping other sources", async () => {
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "git_github_repositories") return ["github/repo"];
      if (command === "git_github_work_items") return [];
      if (command === "bitbucket_status") return { connected: true };
      if (command.endsWith("_status")) return { connected: false };
      if (command === "bitbucket_repo") return "acme/web";
      throw new Error("Bitbucket credentials are invalid or lack permission");
    });

    const result = await listInboxItems(projects, query);

    expect(result.items).toEqual([]);
    expect(result.errors.bitbucket).toContain("invalid or lack permission");
  });

  it("isolates an unreadable Bitbucket config from other providers", async () => {
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "git_github_repositories") return ["github/repo"];
      if (command === "git_github_work_items")
        return [workItem("github/repo", "pr")];
      if (command === "bitbucket_status")
        throw new Error("Could not read the Bitbucket settings");
      if (command.endsWith("_status")) return { connected: false };
      throw new Error(`Unexpected command: ${command}`);
    });

    const result = await listInboxItems(projects, query);

    expect(result.items).toHaveLength(1);
    expect(result.errors.bitbucket).toContain("Bitbucket settings");
  });
});
