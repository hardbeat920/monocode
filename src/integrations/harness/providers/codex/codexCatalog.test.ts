import { beforeEach, expect, it, vi } from "vitest";

const mock = vi.hoisted(() => ({
  handlers: new Map<string, (line: string) => void>(),
  sent: [] as { method: string; params?: unknown }[],
  response: {} as unknown,
  error: false,
  release: vi.fn(),
  spawn: vi.fn(async (..._args: unknown[]) => undefined),
  kill: vi.fn(async (_id: string) => undefined),
}));

vi.mock("../../../../platform/tauri/fs", () => ({
  homeDir: async () => "/home",
}));
vi.mock("../../core/child", () => ({
  acquireHarnessBridge: async () => mock.release,
  resolveCodexBinary: async () => ({ path: "/codex" }),
  spawnChild: mock.spawn,
  killChild: mock.kill,
  watchChild: (id: string, line: (line: string) => void) =>
    mock.handlers.set(id, line),
  unwatchChild: (id: string) => mock.handlers.delete(id),
  writeChild: async (childId: string, line: string) => {
    const request = JSON.parse(line);
    mock.sent.push(request);
    if (request.id === undefined) return;
    const reply =
      request.method === "skills/list"
        ? mock.error
          ? { error: { message: "Unsupported method" } }
          : { result: mock.response }
        : { result: {} };
    mock.handlers.get(childId)?.(JSON.stringify({ id: request.id, ...reply }));
  },
}));

import {
  discoverCodexPluginSkills,
  parseCodexPluginSkills,
} from "./codexCatalog";

const skill = {
  name: "ponytail:ponytail-help",
  description: "Ponytail help",
  path: "/custom-codex/plugins/cache/ponytail/5.2.0/skills/ponytail-help/SKILL.md",
  pluginId: "ponytail@ponytail",
  scope: "user",
  enabled: true,
};
const response = (skills: unknown[]) => ({
  data: [{ cwd: "/repo", skills, errors: [] }],
});

beforeEach(() => {
  vi.clearAllMocks();
  mock.handlers.clear();
  mock.sent.length = 0;
  mock.error = false;
  mock.response = response([skill]);
});

it("uses Codex discovery without account login or starting a thread, then releases its own probe", async () => {
  expect(await discoverCodexPluginSkills("/repo")).toEqual([
    {
      name: skill.name,
      description: skill.description,
      path: skill.path,
      scope: "user",
      source: "codex",
    },
  ]);
  expect(mock.sent.map((request) => request.method)).toEqual([
    "initialize",
    "initialized",
    "skills/list",
  ]);
  expect(mock.sent.at(-1)?.params).toEqual({
    cwds: ["/repo"],
    forceReload: true,
  });
  expect(mock.spawn).toHaveBeenCalledWith(
    expect.stringContaining("monocode-codex-probe-"),
    "/codex",
    ["app-server"],
    "/repo",
    undefined,
    "codex",
  );
  expect(mock.kill).toHaveBeenCalledOnce();
  expect(mock.release).toHaveBeenCalledOnce();
  expect(mock.handlers.size).toBe(0);
});

it("cleans up when an older CLI does not support discovery", async () => {
  mock.error = true;
  await expect(discoverCodexPluginSkills("/repo")).rejects.toThrow(
    "Unsupported method",
  );
  expect(mock.kill).toHaveBeenCalledOnce();
  expect(mock.release).toHaveBeenCalledOnce();
  expect(mock.handlers.size).toBe(0);
});

it("keeps plugin namespaces and exact source paths while deduplicating repeated entries", () => {
  const other = {
    ...skill,
    name: "pstack-for-codex:ponytail-help",
    pluginId: "pstack-for-codex@local",
  };
  expect(
    parseCodexPluginSkills(response([skill, skill, other]), "/repo").map(
      (s) => s.name,
    ),
  ).toEqual([skill.name, other.name]);
});

it("omits disabled skills/plugins, standalone rows, and malformed entries without losing valid rows", () => {
  const rows = [
    null,
    {},
    { ...skill, enabled: false },
    { ...skill, enabled: undefined },
    { ...skill, pluginId: null },
    { ...skill, name: "../bad" },
    { ...skill, path: "relative/SKILL.md" },
    { ...skill, path: "/missing/path" },
    skill,
  ];
  expect(parseCodexPluginSkills(response(rows), "/repo")).toHaveLength(1);
  for (const malformed of [
    null,
    {},
    { data: [null, { cwd: "/repo", skills: null }] },
  ]) {
    expect(parseCodexPluginSkills(malformed, "/repo")).toEqual([]);
  }
  expect(parseCodexPluginSkills(response([skill]), "/other")).toEqual([]);
});
