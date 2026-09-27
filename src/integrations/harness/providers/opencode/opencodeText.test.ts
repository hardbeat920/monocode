import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { HarnessEvent } from "../../core/types";

let onStdout: ((line: string) => void) | undefined;
let onSseEvent: ((event: Record<string, unknown>) => void) | undefined;
let finishPrompt:
  ((value: { status: number; body: string }) => void) | undefined;
let promptStarted = false;

const harnessHttp = vi.fn(
  async (input: {
    url: string;
    method: string;
  }): Promise<{ status: number; body: string }> => {
    const url = new URL(input.url);
    if (input.method === "POST" && url.pathname === "/session") {
      return { status: 200, body: JSON.stringify({ id: "text_session" }) };
    }
    if (
      input.method === "POST" &&
      url.pathname === "/session/text_session/message"
    ) {
      promptStarted = true;
      return new Promise((resolve) => {
        finishPrompt = resolve;
      });
    }
    return { status: 204, body: "" };
  },
);

vi.mock("../../core/child", () => ({
  closeHarnessSse: async () => undefined,
  execChild: async () => "opencode 1.14.19",
  freeHarnessPort: async () => 4096,
  harnessHttp,
  killChild: async () => undefined,
  openHarnessSse: async () => undefined,
  resolveOpenCodeBinary: async () => ({ path: "/fake/opencode" }),
  spawnChild: async () => {
    onStdout?.("opencode server listening on http://127.0.0.1:4096");
  },
  unwatchChild: () => undefined,
  watchChild: (_id: string, stdout: (line: string) => void) => {
    onStdout = stdout;
  },
  watchSse: (_id: string, event: (data: string) => void) => {
    onSseEvent = (value) => event(JSON.stringify(value));
  },
}));

const { runOpenCodeTextPrompt, stopOpenCodeTextPrompt } =
  await import("./opencodeText");

async function waitFor(predicate: () => boolean, label: string) {
  for (let index = 0; index < 200; index += 1) {
    if (predicate()) return;
    await new Promise((resolve) => setTimeout(resolve, 5));
  }
  throw new Error(`timed out waiting for ${label}`);
}

function message(id: string, role: "assistant" | "user") {
  onSseEvent?.({
    type: "message.updated",
    properties: {
      info: { id, role, sessionID: "text_session" },
    },
  });
}

function part(messageID: string, text: string, ended = false) {
  onSseEvent?.({
    type: "message.part.updated",
    properties: {
      part: {
        id: `part_${messageID}`,
        messageID,
        sessionID: "text_session",
        type: "text",
        text,
        time: ended ? { start: 1, end: 2 } : { start: 1 },
      },
    },
  });
}

function delta(messageID: string, text: string) {
  onSseEvent?.({
    type: "message.part.delta",
    properties: {
      sessionID: "text_session",
      partID: `part_${messageID}`,
      delta: text,
    },
  });
}

beforeEach(() => {
  onStdout = undefined;
  onSseEvent = undefined;
  finishPrompt = undefined;
  promptStarted = false;
  harnessHttp.mockClear();
});

afterEach(async () => {
  await stopOpenCodeTextPrompt();
});

it("forwards only incremental OpenCode assistant text", async () => {
  const events: HarnessEvent[] = [];
  const result = runOpenCodeTextPrompt({
    cwd: "/repo",
    model: "openrouter/anthropic/claude-haiku",
    prompt: "question",
    onEvent: (event) => events.push(event),
  });

  await waitFor(() => promptStarted, "prompt");
  message("user_message", "user");
  part("user_message", "question");
  message("assistant_message", "assistant");
  part("assistant_message", "Hel");
  part("assistant_message", "Hello");
  finishPrompt?.({
    status: 200,
    body: JSON.stringify({
      info: {},
      parts: [{ type: "text", text: "Hello" }],
    }),
  });

  await expect(result).resolves.toBe("Hello");
  expect(events).toEqual([
    { type: "message.delta", text: "Hel" },
    { type: "message.delta", text: "lo" },
  ]);
});

it("does not replay a delta after a stale snapshot", async () => {
  const events: HarnessEvent[] = [];
  const result = runOpenCodeTextPrompt({
    cwd: "/repo",
    model: "openrouter/anthropic/claude-haiku",
    prompt: "question",
    onEvent: (event) => events.push(event),
  });

  await waitFor(() => promptStarted, "prompt");
  message("assistant_message", "assistant");
  part("assistant_message", "Hello");
  part("assistant_message", "Hel");
  delta("assistant_message", "!");
  message("assistant_message", "assistant");
  finishPrompt?.({
    status: 200,
    body: JSON.stringify({
      info: {},
      parts: [{ type: "text", text: "Hello!" }],
    }),
  });

  await expect(result).resolves.toBe("Hello!");
  expect(events).toEqual([
    { type: "message.delta", text: "Hello" },
    { type: "message.delta", text: "!" },
  ]);
});

it("does not replay a delta after an out-of-order completed snapshot", async () => {
  const events: HarnessEvent[] = [];
  const result = runOpenCodeTextPrompt({
    cwd: "/repo",
    model: "openrouter/anthropic/claude-haiku",
    prompt: "question",
    onEvent: (event) => events.push(event),
  });

  await waitFor(() => promptStarted, "prompt");
  message("assistant_message", "assistant");
  part("assistant_message", "Hello", true);
  part("assistant_message", "");
  delta("assistant_message", "lo");
  finishPrompt?.({
    status: 200,
    body: JSON.stringify({
      info: {},
      parts: [{ type: "text", text: "Hello" }],
    }),
  });

  await expect(result).resolves.toBe("Hello");
  expect(events).toEqual([{ type: "message.delta", text: "Hello" }]);
});

it("buffers a delta that arrives before its part snapshot", async () => {
  const events: HarnessEvent[] = [];
  const result = runOpenCodeTextPrompt({
    cwd: "/repo",
    model: "openrouter/anthropic/claude-haiku",
    prompt: "question",
    onEvent: (event) => events.push(event),
  });

  await waitFor(() => promptStarted, "prompt");
  delta("assistant_message", "Hel");
  message("assistant_message", "assistant");
  part("assistant_message", "");
  delta("assistant_message", "lo");
  part("assistant_message", "Hello", true);
  finishPrompt?.({
    status: 200,
    body: JSON.stringify({
      info: {},
      parts: [{ type: "text", text: "Hello" }],
    }),
  });

  await expect(result).resolves.toBe("Hello");
  expect(events).toEqual([
    { type: "message.delta", text: "Hel" },
    { type: "message.delta", text: "lo" },
  ]);
});
