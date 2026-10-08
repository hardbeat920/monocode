import { afterEach, beforeEach, expect, it, vi } from "vitest";

const sent: string[] = [];
let onLine: ((line: string) => void) | undefined;
const execChild = vi.fn(async () => "");

vi.mock("../../../../platform/tauri/fs", () => ({
  homeDir: async () => "/home/test",
}));

vi.mock("../../core/child", () => ({
  resolveCursorBinary: async () => ({ path: "/fake/cursor-agent" }),
  spawnChild: async () => undefined,
  killChild: async () => undefined,
  unwatchChild: () => undefined,
  execChild: (...args: unknown[]) => execChild(...args),
  watchChild: (_id: string, line: (value: string) => void) => {
    onLine = line;
  },
  writeChild: async (_id: string, line: string) => {
    sent.push(line);
  },
}));

const { discoverCursorModels } = await import("./cursorCatalog");

function messages() {
  return sent.map((line) => JSON.parse(line) as Record<string, unknown>);
}

function outbound(method: string) {
  return messages().find((message) => message.method === method);
}

function reply(method: string, result: unknown) {
  const request = outbound(method);
  onLine?.(JSON.stringify({ jsonrpc: "2.0", id: request?.id, result }));
}

async function waitFor(predicate: () => boolean, label: string) {
  for (let index = 0; index < 200; index += 1) {
    if (predicate()) return;
    await new Promise((resolve) => setTimeout(resolve, 5));
  }
  throw new Error(`timed out waiting for ${label}`);
}

beforeEach(() => {
  sent.length = 0;
  onLine = undefined;
  execChild.mockReset();
  execChild.mockResolvedValue("");
});

afterEach(() => {
  sent.length = 0;
  onLine = undefined;
});

it("lists Cursor models without opening the login browser", async () => {
  const models = discoverCursorModels("/repo");

  await waitFor(() => !!outbound("initialize"), "initialize");
  reply("initialize", {});
  await waitFor(
    () => !!outbound("cursor/list_available_models"),
    "list_available_models",
  );
  reply("cursor/list_available_models", {
    models: [
      { id: "composer-2.5", name: "Composer 2.5" },
      { value: "grok-4.6", name: "Grok 4.6" },
    ],
  });

  await expect(models).resolves.toEqual([
    {
      id: "cursor:composer-2.5",
      harness: "cursor",
      name: "Composer 2.5",
      nativeId: "composer-2.5",
    },
    {
      id: "cursor:grok-4.6",
      harness: "cursor",
      name: "Grok 4.6",
      nativeId: "grok-4.6",
    },
  ]);
  expect(messages().some((message) => message.method === "authenticate")).toBe(
    false,
  );
  expect(execChild).not.toHaveBeenCalled();
});

it("falls back to --list-models without authenticating", async () => {
  execChild.mockResolvedValue("composer-2.5 - Composer 2.5\n");
  const models = discoverCursorModels("/repo");

  await waitFor(() => !!outbound("initialize"), "initialize");
  reply("initialize", {});
  await waitFor(
    () => !!outbound("cursor/list_available_models"),
    "list_available_models",
  );
  reply("cursor/list_available_models", { models: [] });
  await waitFor(() => !!outbound("session/new"), "session/new");
  reply("session/new", {});

  await expect(models).resolves.toEqual([
    {
      id: "cursor:composer-2.5",
      harness: "cursor",
      name: "Composer 2.5",
      nativeId: "composer-2.5",
    },
  ]);
  expect(messages().some((message) => message.method === "authenticate")).toBe(
    false,
  );
  expect(execChild).toHaveBeenCalledWith(
    "/fake/cursor-agent",
    ["--list-models"],
    "/repo",
    "cursor",
  );
});
