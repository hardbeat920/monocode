import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { HarnessEvent } from "../../core/types";

const cleanup = vi.hoisted(() => ({ kill: vi.fn(async () => undefined) }));
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
  killChild: cleanup.kill,
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
  cleanup.kill.mockClear();
});

afterEach(async () => {
  await stopOpenCodeTextPrompt();
});

it("forwards OpenCode assistant part snapshots", async () => {
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
  expect(harnessHttp).toHaveBeenCalledWith(
    expect.objectContaining({
      method: "DELETE",
      url: "http://127.0.0.1:4096/session/text_session?directory=%2Frepo",
    }),
  );
  expect(events).toEqual([snapshot("Hel"), snapshot("Hello")]);
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
  expect(events).toEqual([snapshot("Hello"), snapshot("Hello!")]);
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
  expect(events).toEqual([snapshot("Hello", false)]);
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
    snapshot("Hel"),
    snapshot("Hello"),
    snapshot("Hello", false),
  ]);
});

it("cleans up when cancellation arrives during session creation", async () => {
  const controller = new AbortController();
  harnessHttp.mockImplementationOnce(async () => {
    controller.abort();
    return { status: 200, body: JSON.stringify({ id: "text_session" }) };
  });
  await expect(
    runOpenCodeTextPrompt({
      cwd: "/repo",
      model: "openai/fixture",
      prompt: "question",
      signal: controller.signal,
    }),
  ).rejects.toHaveProperty("name", "AbortError");
  expect(cleanup.kill).toHaveBeenCalledWith("monocode-opencode-text");
  expect(promptStarted).toBe(false);
});

function snapshot(text: string, streaming = true): HarnessEvent {
  return {
    type: "message.part",
    partId: "part_assistant_message",
    text,
    reasoning: false,
    streaming,
  };
}

it("replaces streamed text with a shorter completed part", async () => {
  const events: HarnessEvent[] = [];
  const result = runOpenCodeTextPrompt({
    cwd: "/repo",
    model: "openai/fixture",
    prompt: "question",
    onEvent: (event) => events.push(event),
  });
  await waitFor(() => promptStarted, "prompt");
  message("assistant_message", "assistant");
  part("assistant_message", "A longer provisional answer");
  part("assistant_message", "Final", true);
  finishPrompt?.({
    status: 200,
    body: JSON.stringify({
      info: {},
      parts: [{ type: "text", text: "Final" }],
    }),
  });
  await expect(result).resolves.toBe("Final");
  expect(events).toEqual([
    snapshot("A longer provisional answer"),
    snapshot("Final", false),
  ]);
});

it("reconciles the completed HTTP response before closing the stream", async () => {
  const events: HarnessEvent[] = [];
  const result = runOpenCodeTextPrompt({
    cwd: "/repo",
    model: "openai/fixture",
    prompt: "question",
    onEvent: (event) => events.push(event),
  });
  await waitFor(() => promptStarted, "prompt");
  message("assistant_message", "assistant");
  part("assistant_message", "A longer provisional answer");
  finishPrompt?.({
    status: 200,
    body: JSON.stringify({
      info: {
        id: "assistant_message",
        role: "assistant",
        sessionID: "text_session",
      },
      parts: [
        {
          id: "part_assistant_message",
          messageID: "assistant_message",
          type: "text",
          text: "Final",
        },
      ],
    }),
  });
  await expect(result).resolves.toBe("Final");
  expect(events).toEqual([
    snapshot("A longer provisional answer"),
    snapshot("Final", false),
  ]);
});

it("kills the owned server when its abort request fails", async () => {
  const result = runOpenCodeTextPrompt({
    cwd: "/repo",
    model: "openai/fixture",
    prompt: "question",
  });
  await waitFor(() => promptStarted, "prompt");
  harnessHttp.mockImplementationOnce(async () => ({
    status: 503,
    body: "server unavailable",
  }));
  finishPrompt?.({
    status: 200,
    body: JSON.stringify({
      info: {},
      parts: [{ type: "text", text: "Answer" }],
    }),
  });
  await expect(result).resolves.toBe("Answer");
  expect(cleanup.kill).toHaveBeenCalledWith("monocode-opencode-text");
});

it("deletes the generated-text session when the provider returns an error", async () => {
  const result = runOpenCodeTextPrompt({
    cwd: "/repo",
    model: "openai/test",
    prompt: "Generate text",
  });
  await waitFor(() => promptStarted, "prompt");
  finishPrompt?.({
    status: 200,
    body: JSON.stringify({ info: { error: { message: "Generation failed" } } }),
  });
  await expect(result).rejects.toThrow("Generation failed");
  expect(harnessHttp).toHaveBeenCalledWith(
    expect.objectContaining({
      method: "DELETE",
      url: "http://127.0.0.1:4096/session/text_session?directory=%2Frepo",
    }),
  );
});

it("deletes the generated-text session when generation is cancelled", async () => {
  const controller = new AbortController();
  const result = runOpenCodeTextPrompt({
    cwd: "/repo",
    model: "openai/test",
    prompt: "Generate text",
    signal: controller.signal,
  });
  await waitFor(() => promptStarted, "prompt");
  controller.abort();
  await expect(result).rejects.toThrow("By-the-way request cancelled");
  expect(harnessHttp).toHaveBeenCalledWith(
    expect.objectContaining({
      method: "DELETE",
      url: "http://127.0.0.1:4096/session/text_session?directory=%2Frepo",
    }),
  );
});
