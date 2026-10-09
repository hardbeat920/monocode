import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  close: vi.fn(),
  request: vi.fn(),
  resolveBinary: vi.fn(),
  spawnChild: vi.fn(),
  killChild: vi.fn(),
  frames: [] as Array<(record: Record<string, unknown>) => void>,
  exits: [] as Array<(code: number) => void>,
}));

vi.mock("../../core/child", () => ({
  preparePiBackgroundBridge: async () => "/fake/bridge.mjs",
  killChild: mocks.killChild,
  resolveOmpBinary: vi.fn(),
  resolvePiBinary: mocks.resolveBinary,
  spawnChild: mocks.spawnChild,
  unwatchChild: vi.fn(),
  watchChild: vi.fn((_id, _line, exit) => mocks.exits.push(exit)),
  writeChild: vi.fn(),
}));

vi.mock("./piClient", () => ({
  PiRpc: class {
    constructor(
      _sessionId: string,
      onFrame: (record: Record<string, unknown>) => void,
    ) {
      mocks.frames.push(onFrame);
    }

    request = mocks.request;
    close = mocks.close;
    cancelRequest = vi.fn();
    pushLine = vi.fn();
  },
}));

import { cancelPiTurn, compactPiContext, sendPiTurn, steerPiTurn, stopPiSession } from "./pi";
import type { HarnessEvent } from "../../core/types";

describe("Pi live session", () => {
  beforeEach(() => {
    mocks.close.mockReset();
    mocks.request.mockReset();
    mocks.resolveBinary.mockReset();
    mocks.spawnChild.mockReset();
    mocks.killChild.mockReset();
    mocks.frames.length = 0;
    mocks.exits.length = 0;
    mocks.resolveBinary.mockResolvedValue({ path: "/fake/pi" });
    mocks.spawnChild.mockResolvedValue(undefined);
    mocks.killChild.mockResolvedValue(undefined);
    mocks.request.mockImplementation(
      async (command: Record<string, unknown>) => {
        if (command.type === "get_state") {
          return {
            data: {
              sessionId: "pi_session",
              model: { contextWindow: 200_000 },
            },
          };
        }
        if (command.type === "compact") {
          return { data: { estimatedTokensAfter: 32_000 } };
        }
        return { data: {} };
      },
    );
  });

  it("publishes the resolved Pi default model for provider usage", async () => {
    mocks.request.mockImplementation(async (command: Record<string, unknown>) => {
      if (command.type === "get_state") return { data: {
        sessionId: "pi_default",
        model: { provider: "openai-codex", id: "gpt-5.4", contextWindow: 200_000 },
      } };
      return { data: {} };
    });
    const events: HarnessEvent[] = [];
    await compactPiContext({
      sessionId: "pi-default", cwd: "/repo", model: "pi:default",
      runtimeMode: "supervised", onEvent: event => events.push(event),
    });
    expect(events).toContainEqual({
      type: "session.configChanged", model: "pi:openai-codex/gpt-5.4",
    });
    await stopPiSession("pi-default");
  });

  it("does not publish an intermediate default when explicit model selection fails", async () => {
    mocks.request.mockImplementation(async (command: Record<string, unknown>) => {
      if (command.type === "get_state") return { data: {
        sessionId: "pi_explicit",
        model: { provider: "anthropic", id: "claude-sonnet-5", contextWindow: 200_000 },
      } };
      if (command.type === "set_model") throw new Error("Model unavailable");
      return { data: {} };
    });
    const events: HarnessEvent[] = [];
    await expect(compactPiContext({
      sessionId: "pi-explicit", cwd: "/repo", model: "pi:openai-codex/gpt-5.4",
      runtimeMode: "supervised", onEvent: event => events.push(event),
    })).rejects.toThrow("Model unavailable");
    expect(events.filter(event => event.type === "session.configChanged")).toEqual([]);
    expect(mocks.request).toHaveBeenCalledWith({ type: "set_model", provider: "openai-codex", modelId: "gpt-5.4" });
    await stopPiSession("pi-explicit");
  });

  it("uses the compact RPC command and publishes the post-compact estimate", async () => {
    const events: HarnessEvent[] = [];

    await compactPiContext({
      sessionId: "pi-compact",
      cwd: "/repo",
      model: "pi:default",
      runtimeMode: "supervised",
      onEvent: (event) => events.push(event),
    });

    expect(mocks.request).toHaveBeenCalledWith(
      { type: "compact" },
      30 * 60_000,
    );
    expect(events).toContainEqual({
      type: "context",
      used: 32_000,
      window: 200_000,
    });
    await stopPiSession("pi-compact");
  });

  it("publishes readable Ponytail status and extension notifications", async () => {
    const events: HarnessEvent[] = [];
    await compactPiContext({
      sessionId: "pi-ansi",
      cwd: "/repo",
      model: "pi:default",
      runtimeMode: "supervised",
      onEvent: (event) => events.push(event),
    });
    const frame = mocks.frames[0]!;
    frame({
      type: "extension_ui_request",
      id: "ponytail-status",
      method: "setStatus",
      statusKey: "ponytail",
      statusText:
        "\u001b[38;5;241m○\u001b[39m \u001b[38;5;244mponytail:\u001b[39m \u001b[38;5;188m⚡ FULL\u001b[0m",
    });
    frame({
      type: "extension_ui_request",
      id: "plugin-notify",
      method: "notify",
      message: "\u001b[32mPlugin ready\u001b[0m",
    });
    frame({
      type: "extension_ui_request",
      id: "empty-status",
      method: "setStatus",
      statusText: "\u001b[0m",
    });
    expect(events.filter((event) => event.type === "status")).toEqual([
      { type: "status", key: "ponytail", text: "○ ponytail: ⚡ FULL" },
      { type: "status", text: "Plugin ready" },
    ]);
    await stopPiSession("pi-ansi");
  });

  it("publishes animated extension status as one keyed slot", async () => {
    const events: HarnessEvent[] = [];
    await compactPiContext({
      sessionId: "pi-caveman",
      cwd: "/repo",
      model: "pi:default",
      runtimeMode: "supervised",
      onEvent: (event) => events.push(event),
    });
    const frame = mocks.frames[0]!;
    for (const [id, statusText] of [
      ["frame-1", "⠋ \u001b[2mcaveman level: \u001b[0mULTRA"],
      ["frame-2", "⠙ \u001b[2mcaveman level: \u001b[0mULTRA"],
      ["clear", ""],
    ]) {
      frame({
        type: "extension_ui_request",
        id,
        method: "setStatus",
        statusKey: "caveman",
        statusText,
      });
    }
    expect(events.filter((event) => event.type === "status")).toEqual([
      { type: "status", key: "caveman", text: "⠋ caveman level: ULTRA" },
      { type: "status", key: "caveman", text: "⠙ caveman level: ULTRA" },
      { type: "status", key: "caveman", text: "" },
    ]);
    await stopPiSession("pi-caveman");
  });
});


describe("Pi extension-triggered runs", () => {
  const input = {
    sessionId: "pi-wake", cwd: "/repo", model: "pi:default",
    runtimeMode: "supervised" as const,
  };
  let events: HarnessEvent[];
  let frame: (record: Record<string, unknown>) => void;

  beforeEach(async () => {
    mocks.frames.length = 0;
    mocks.exits.length = 0;
    mocks.request.mockReset().mockImplementation(async (command) => ({
      data: command.type === "get_state" ? { sessionId: "native-pi" } : {},
    }));
    mocks.resolveBinary.mockResolvedValue({ path: "/fake/pi" });
    events = [];
    await compactPiContext({ ...input, onEvent: event => events.push(event) });
    frame = mocks.frames[0]!;
  });

  afterEach(async () => { await stopPiSession(input.sessionId); });

  async function submit() {
    let settled = false;
    const done = sendPiTurn({ ...input, text: "Work", attachments: [],
      onEvent: event => events.push(event),
    }).then(() => { settled = true; });
    await vi.waitFor(() => expect(mocks.request).toHaveBeenCalledWith(
      expect.objectContaining({ type: "prompt" }), expect.any(Number),
    ));
    return { done, settled: () => settled };
  }

  function background(tasks: string[]) {
    frame({ type: "extension_ui_request", method: "setStatus", id: "bridge",
      statusKey: "monocode.pi-background.v1", statusText: JSON.stringify({ version: 1, tasks }),
    });
  }

  it("holds the user turn through background waiting, delivery and parent follow-up", async () => {
    const turn = await submit();
    expect(mocks.spawnChild).toHaveBeenCalledWith(input.sessionId, "/fake/pi",
      expect.arrayContaining(["--extension", "/fake/bridge.mjs"]), "/repo", undefined, "pi");
    frame({ type: "agent_start" });
    background(["pi-subagents"]);
    frame({ type: "agent_settled" });
    await new Promise(resolve => setTimeout(resolve, 0));
    expect(turn.settled()).toBe(false);
    expect(events).toContainEqual({ type: "background.updated", tasks: ["pi-subagents"] });
    // The extension keeps the lease during pending notification delivery.
    frame({ type: "agent_start" });
    background([]);
    await new Promise(resolve => setTimeout(resolve, 0));
    expect(turn.settled()).toBe(false);
    frame({ type: "agent_settled" });
    await turn.done;
  });

  it("retains extension work registered between parent runs and releases without a wake", async () => {
    background(["pi-subagents"]);
    expect(events).toContainEqual({ type: "turn.activity", active: true });
    background([]);
    await vi.waitFor(() => expect(events).toContainEqual({ type: "turn.activity", active: false }));
  });

  it("can prompt while waiting and settles a no-wake completion", async () => {
    const turn = await submit();
    frame({ type: "agent_start" });
    background(["pi-subagents"]);
    frame({ type: "agent_settled" });
    await steerPiTurn({ ...input, text: "Status?", attachments: [] });
    expect(mocks.request).toHaveBeenLastCalledWith(expect.objectContaining({ type: "prompt", message: "Status?" }));
    background([]);
    await turn.done;
  });

  it("ignores malformed bridge data and late updates after cancellation", async () => {
    const turn = await submit();
    frame({ type: "agent_start" });
    background(["pi-subagents"]);
    frame({ type: "extension_ui_request", method: "setStatus", id: "bad",
      statusKey: "monocode.pi-background.v1", statusText: "{bad json",
    });
    frame({ type: "agent_settled" });
    expect(turn.settled()).toBe(false);
    await cancelPiTurn(input.sessionId);
    await turn.done;
    events.length = 0;
    background(["pi-subagents"]);
    expect(events).toEqual([]);
  });

  it("waits for agent_settled rather than ending at a low-level agent_end", async () => {
    const turn = await submit();
    frame({ type: "agent_start" });
    frame({ type: "agent_end", willRetry: false });
    await new Promise(resolve => setTimeout(resolve, 0));
    expect(turn.settled()).toBe(false);
    expect(mocks.request).not.toHaveBeenCalledWith(
      { type: "get_session_stats" }, expect.any(Number),
    );
    frame({ type: "agent_settled" });
    await turn.done;
    expect(events).toContainEqual({ type: "turn.activity", active: false });
  });

  it("publishes a later autonomous run and leaves the next user prompt usable", async () => {
    const turn = await submit();
    frame({ type: "agent_start" });
    frame({ type: "agent_settled" });
    await turn.done;
    events.length = 0;

    // The extension has delivered its result and wakes Pi without sendPiTurn.
    frame({ type: "agent_start" });
    expect(events).toContainEqual({ type: "turn.activity", active: true });
    await steerPiTurn({ ...input, text: "Keep going", attachments: [] });
    expect(mocks.request).toHaveBeenCalledWith(expect.objectContaining({ type: "steer" }));
    frame({ type: "agent_settled" });
    await vi.waitFor(() => expect(events).toContainEqual({ type: "turn.activity", active: false }));

    mocks.request.mockClear();
    const next = await submit();
    expect(next.settled()).toBe(false);
    frame({ type: "agent_start" });
    frame({ type: "agent_settled" });
    await next.done;
  });

  it("preserves a wake arriving while the submitted turn is unwinding", async () => {
    let wake = true;
    const done = sendPiTurn({ ...input, text: "Work", attachments: [], onEvent: event => {
      events.push(event);
      if (wake && event.type === "turn.activity" && !event.active) {
        wake = false;
        queueMicrotask(() => frame({ type: "agent_start" }));
      }
    } });
    await vi.waitFor(() => expect(mocks.request).toHaveBeenCalledWith(
      expect.objectContaining({ type: "prompt" }), expect.any(Number),
    ));
    frame({ type: "agent_start" });
    frame({ type: "agent_settled" });
    await done;
    await expect(steerPiTurn({ ...input, text: "Continue", attachments: [] })).resolves.toBeUndefined();
    frame({ type: "agent_settled" });
    await vi.waitFor(() => expect(events.filter(event =>
      event.type === "turn.activity" && !event.active,
    )).toHaveLength(2));
  });

  it("does not let a previous stats request settle a newly started run", async () => {
    const turn = await submit();
    let stats!: (value: Record<string, unknown>) => void;
    mocks.request.mockImplementationOnce(() => new Promise(resolve => { stats = resolve; }));
    frame({ type: "agent_settled" });
    frame({ type: "agent_start" });
    stats({ data: {} });
    await new Promise(resolve => setTimeout(resolve, 0));
    expect(turn.settled()).toBe(false);
    expect(events).not.toContainEqual({ type: "turn.activity", active: false });
    frame({ type: "agent_settled" });
    await turn.done;
  });

  it("settles a failed autonomous run and clears activity when Pi exits", async () => {
    frame({ type: "agent_start" });
    frame({ type: "message_end", message: {
      role: "assistant", stopReason: "error", errorMessage: "Provider unavailable", content: [],
    } });
    frame({ type: "agent_settled" });
    await vi.waitFor(() => expect(events).toContainEqual({ type: "turn.activity", active: false }));
    expect(events).toContainEqual({ type: "session.error", message: "Provider unavailable" });
    events.length = 0;
    frame({ type: "agent_start" });
    mocks.exits[0]!(1);
    expect(events).toContainEqual({ type: "turn.activity", active: false });
    expect(events).toContainEqual({ type: "session.ended", code: 1 });
  });

  it("cancels an autonomous run and ignores late lifecycle frames", async () => {
    frame({ type: "agent_start" });
    await cancelPiTurn(input.sessionId);
    expect(mocks.request).toHaveBeenCalledWith({ type: "abort" }, 5000);
    expect(events).toContainEqual({ type: "turn.activity", active: false });
    events.length = 0;
    frame({ type: "agent_start" });
    frame({ type: "agent_settled" });
    expect(events).toEqual([]);
  });
});
