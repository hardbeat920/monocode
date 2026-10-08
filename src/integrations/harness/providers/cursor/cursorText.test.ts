import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { HarnessEvent } from "../../core/types";

const sent: string[] = [];
let onLine: ((line: string) => void) | undefined;

vi.mock("../../core/child", () => ({
  resolveCursorBinary: async () => ({ path: "/fake/cursor-agent" }),
  spawnChild: async () => undefined,
  killChild: async () => undefined,
  unwatchChild: () => undefined,
  watchChild: (_id: string, line: (value: string) => void) => {
    onLine = line;
  },
  writeChild: async (_id: string, line: string) => {
    sent.push(line);
  },
}));

const { runCursorTextPrompt, stopCursorTextPrompt } =
  await import("./cursorText");

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
});

afterEach(async () => {
  await stopCursorTextPrompt();
});

it("forwards Cursor text deltas without duplicating snapshots", async () => {
  const events: HarnessEvent[] = [];
  const result = runCursorTextPrompt({
    cwd: "/repo",
    prompt: "question",
    onEvent: (event) => events.push(event),
  });

  await waitFor(() => !!outbound("initialize"), "initialize");
  reply("initialize", {});
  await waitFor(() => !!outbound("session/new"), "session/new");
  reply("session/new", {
    sessionId: "cursor_text",
    configOptions: [{ id: "model", category: "model" }],
  });
  await waitFor(() => !!outbound("session/set_mode"), "session/set_mode");
  reply("session/set_mode", {});
  await waitFor(
    () => !!outbound("session/set_config_option"),
    "session/set_config_option",
  );
  reply("session/set_config_option", {});
  await waitFor(() => !!outbound("session/prompt"), "session/prompt");

  onLine?.(
    JSON.stringify({
      jsonrpc: "2.0",
      method: "session/update",
      params: {
        sessionId: "cursor_text",
        update: { sessionUpdate: "agent_message_chunk", content: "Hel" },
      },
    }),
  );
  onLine?.(
    JSON.stringify({
      jsonrpc: "2.0",
      method: "session/update",
      params: {
        sessionId: "cursor_text",
        update: { sessionUpdate: "agent_message", content: "Hello" },
      },
    }),
  );
  reply("session/prompt", {});

  await expect(result).resolves.toBe("Hello");
  expect(events).toEqual([
    { type: "message.delta", text: "Hel" },
    { type: "message.delta", text: "lo" },
  ]);
  // `cursor_login` opens a browser; background text must never send it.
  expect(outbound("authenticate")).toBeUndefined();
});

it("fails with Cursor's auth error instead of opening the login browser", async () => {
  const result = runCursorTextPrompt({ cwd: "/repo", prompt: "title" });

  await waitFor(() => !!outbound("initialize"), "initialize");
  reply("initialize", {});
  await waitFor(() => !!outbound("session/new"), "session/new");
  const request = outbound("session/new");
  onLine?.(
    JSON.stringify({
      jsonrpc: "2.0",
      id: request?.id,
      error: {
        code: -32000,
        message:
          "Authentication required. Please run 'cursor-agent login' first, then call authenticate() with methodId 'cursor_login'.",
      },
    }),
  );

  await expect(result).rejects.toThrow(/Authentication required/);
  expect(outbound("authenticate")).toBeUndefined();
});
