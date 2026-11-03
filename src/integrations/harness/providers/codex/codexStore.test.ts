import { beforeEach, expect, it, vi } from "vitest";

const mock = vi.hoisted(() => ({
  handlers: new Map<string, (line: string) => void>(),
  sent: [] as { method: string; params?: Record<string, unknown> }[],
  archived: false,
  missing: false,
  release: vi.fn(),
  spawn: vi.fn(async (..._args: unknown[]) => undefined),
  kill: vi.fn(async (_id: string) => undefined),
  prepare: vi.fn(async (..._args: unknown[]) => ({
    home: "/mono/default",
    hasThread: false,
  })),
  copy: vi.fn(async (..._args: unknown[]) => undefined),
}));

vi.mock("../../core/child", () => ({
  acquireHarnessBridge: async () => mock.release,
  prepareMonoCodexStore: mock.prepare,
  copyMonoCodexThreads: mock.copy,
  spawnChild: mock.spawn,
  killChild: mock.kill,
  watchChild: (id: string, line: (line: string) => void) =>
    mock.handlers.set(id, line),
  unwatchChild: (id: string) => mock.handlers.delete(id),
  writeChild: async (childId: string, line: string) => {
    const request = JSON.parse(line);
    mock.sent.push(request);
    if (request.id === undefined) return;
    let result: unknown = {};
    if (request.method === "config/read") {
      result = {
        config: {
          model: null,
          instructions: "Shared account instructions",
          sqlite_home: "/ordinary",
          model_instructions_file: "/ordinary/instructions.md",
        },
      };
    } else if (request.method === "thread/read") {
      if (mock.missing) {
        mock.handlers.get(childId)?.(
          JSON.stringify({
            id: request.id,
            error: { message: "thread not found" },
          }),
        );
        return;
      }
      const id = request.params.threadId;
      result = {
        thread: {
          id,
          path: `/ordinary/${mock.archived ? "archived_sessions" : "sessions"}/rollout-${id}.jsonl`,
        },
      };
    } else if (request.method === "thread/list") {
      const id = request.params.archived
        ? "archived-child"
        : request.params.cursor
          ? "second-child"
          : "child";
      result = {
        data: [{ id }],
        nextCursor:
          !request.params.archived && !request.params.cursor ? "page-2" : null,
      };
    } else if (request.method === "thread/archive") {
      mock.archived = true;
    } else if (request.method === "thread/unarchive") {
      mock.archived = false;
    }
    mock.handlers.get(childId)?.(JSON.stringify({ id: request.id, result }));
  },
}));

import { codexConfigOverrides, prepareCodexMonoContext } from "./codexStore";
const input = {
  sessionId: "mono-chat",
  path: "/codex",
  cwd: "/project",
  providerAccountId: "account",
};

beforeEach(() => {
  mock.handlers.clear();
  mock.sent.length = 0;
  mock.archived = false;
  mock.missing = false;
  vi.clearAllMocks();
  mock.prepare.mockResolvedValue({ home: "/mono/default", hasThread: false });
  mock.copy.mockResolvedValue(undefined);
});

it("omits nested unset settings while preserving typed account configuration", () => {
  expect(
    codexConfigOverrides(
      {
        model: null,
        shell_environment_policy: {
          inherit: null,
          include_only: null,
          exclude: ["TOKEN"],
          set: { MONO: "yes" },
        },
        agents: {
          helper: { config_file: "/account/helper.toml", description: null },
        },
        enabled: false,
      },
      "/mono",
    ),
  ).toEqual({
    shell_environment_policy: { exclude: ["TOKEN"], set: { MONO: "yes" } },
    agents: { helper: { config_file: "/account/helper.toml" } },
    enabled: false,
    sqlite_home: "/mono",
  });
});

it("uses the account's resolved settings with an isolated SQLite home", async () => {
  const prepared = await prepareCodexMonoContext(input);
  expect(prepared).toEqual({
    config: {
      instructions: "Shared account instructions",
      model_instructions_file: "/ordinary/instructions.md",
      sqlite_home: "/mono/default",
    },
    hasThread: false,
  });
  expect(mock.spawn).toHaveBeenCalledWith(
    expect.any(String),
    "/codex",
    ["app-server"],
    "/project",
    { provider: "codex", id: "account" },
    "codex",
  );
  expect(mock.sent.map((r) => r.method)).toEqual([
    "initialize",
    "initialized",
    "config/read",
  ]);
  expect(mock.release).toHaveBeenCalledOnce();
  expect(mock.kill).toHaveBeenCalledOnce();
  expect(mock.handlers.size).toBe(0);
});

it("archives the old root and retains all native descendants before resuming privately", async () => {
  const prepared = await prepareCodexMonoContext({
    ...input,
    threadId: "root",
  });
  expect(prepared.hasThread).toBe(true);
  expect(mock.sent.filter((r) => r.method === "thread/list")).toHaveLength(3);
  expect(mock.sent.filter((r) => r.method === "thread/archive")).toEqual([
    {
      id: expect.any(Number),
      method: "thread/archive",
      params: { threadId: "root" },
    },
  ]);
  expect(mock.copy).toHaveBeenCalledWith(
    "account",
    "root",
    [
      "/ordinary/archived_sessions/rollout-root.jsonl",
      "/ordinary/archived_sessions/rollout-child.jsonl",
      "/ordinary/archived_sessions/rollout-second-child.jsonl",
      "/ordinary/archived_sessions/rollout-archived-child.jsonl",
    ],
    "/ordinary",
  );
  expect(
    mock.sent.some((r) =>
      ["thread/start", "turn/start", "thread/resume"].includes(r.method),
    ),
  ).toBe(false);
});

it("does not reopen the ordinary home for an already migrated startup thread", async () => {
  mock.prepare.mockResolvedValue({ home: "/mono/default", hasThread: true });
  await expect(
    prepareCodexMonoContext({
      ...input,
      threadId: "root",
      migrationOnly: true,
    }),
  ).resolves.toEqual({ config: {}, hasThread: true });
  expect(mock.spawn).not.toHaveBeenCalled();
  expect(mock.copy).not.toHaveBeenCalled();
});

it("can migrate an archived legacy root without archiving it twice", async () => {
  mock.archived = true;
  await prepareCodexMonoContext({ ...input, threadId: "root" });
  expect(mock.copy).toHaveBeenCalledOnce();
  expect(mock.sent.some((r) => r.method === "thread/archive")).toBe(false);
});

it("restores the original root if retaining its context fails", async () => {
  mock.copy.mockRejectedValueOnce(new Error("Disk full"));
  await expect(
    prepareCodexMonoContext({ ...input, threadId: "root" }),
  ).rejects.toThrow("Disk full");
  expect(mock.sent.at(-1)).toMatchObject({
    method: "thread/unarchive",
    params: { threadId: "root" },
  });
  expect(mock.archived).toBe(false);
  expect(mock.release).toHaveBeenCalledOnce();
  expect(mock.handlers.size).toBe(0);
  // Failed work releases the lock and can be retried.
  await expect(
    prepareCodexMonoContext({ ...input, threadId: "root" }),
  ).resolves.toMatchObject({ hasThread: true });
});

it("keeps normal recovery available when an old thread was already removed", async () => {
  mock.missing = true;
  await expect(
    prepareCodexMonoContext({ ...input, threadId: "deleted" }),
  ).resolves.toMatchObject({ hasThread: false });
  expect(mock.copy).not.toHaveBeenCalled();
  expect(mock.sent.some((r) => r.method === "thread/archive")).toBe(false);
});

it("coalesces an overlapping startup migration and send preparation", async () => {
  const first = prepareCodexMonoContext({ ...input, threadId: "root" });
  const second = prepareCodexMonoContext({ ...input, threadId: "root" });
  expect(second).toBe(first);
  await Promise.all([first, second]);
  expect(mock.copy).toHaveBeenCalledOnce();
  expect(mock.spawn).toHaveBeenCalledOnce();
});
