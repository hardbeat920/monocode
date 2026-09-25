import { beforeEach, describe, expect, it, vi } from "vitest";

const sent: string[] = [];
let onLine: ((line: string) => void) | undefined;
let onStderr: ((line: string) => void) | undefined;
let onExit: ((code: number | null) => void) | undefined;
let blockCancelWrite = false;
const textFiles = new Map<string, string>();

vi.mock("../../core/child", () => ({
  resolveHermesBinary: async () => ({ path: "/fake/hermes" }),
  spawnChild: async () => undefined,
  killChild: async () => undefined,
  unwatchChild: () => undefined,
  watchChild: (
    _id: string,
    line: (value: string) => void,
    exit: (code: number | null) => void,
    stderr: (value: string) => void,
  ) => {
    onLine = line;
    onExit = exit;
    onStderr = stderr;
  },
  writeChild: async (_id: string, line: string) => {
    sent.push(line);
    if (blockCancelWrite && JSON.parse(line).method === "session/cancel") {
      await new Promise<void>(() => undefined);
    }
  },
}));

vi.mock("../../../../platform/tauri/fs", () => ({
  readTextFile: async (path: string) => {
    const content = textFiles.get(path);
    if (content == null) throw new Error(`missing ${path}`);
    return content;
  },
}));

const {
  bindHermesSession,
  respondHermesApproval,
  sendHermesTurn,
  stopHermesSession,
} = await import("./hermes");
import type { HarnessEvent } from "../../core/types";

const parse = () => sent.map((line) => JSON.parse(line));

function reply(id: number, result: unknown) {
  onLine!(JSON.stringify({ jsonrpc: "2.0", id, result }));
}

async function waitFor(predicate: () => boolean, label: string) {
  for (let index = 0; index < 200; index += 1) {
    if (predicate()) return;
    await new Promise((resolve) => setTimeout(resolve, 5));
  }
  throw new Error(
    `timed out waiting for ${label}; sent=${JSON.stringify(parse().map((message) => message.method ?? `reply:${message.id}`))}`,
  );
}

async function initialize() {
  await waitFor(
    () => parse().some((message) => message.method === "initialize"),
    "initialize",
  );
  const request = parse().find((message) => message.method === "initialize")!;
  reply(request.id, { protocolVersion: 1 });
}

async function newSession() {
  await waitFor(
    () => parse().some((message) => message.method === "session/new"),
    "session/new",
  );
  const request = parse().find((message) => message.method === "session/new")!;
  reply(request.id, {
    sessionId: "hermes-session-1",
    models: { currentModelId: "nous:hermes-4" },
  });
}

describe("Hermes live ACP sequence", () => {
  beforeEach(() => {
    sent.length = 0;
    onLine = undefined;
    onExit = undefined;
    onStderr = undefined;
    blockCancelWrite = false;
    textFiles.clear();
  });

  it("starts Hermes ACP, selects model and mode, and sends attachments", async () => {
    const events: HarnessEvent[] = [];
    const turn = sendHermesTurn({
      sessionId: "hermes-live-new",
      cwd: "/repo",
      model: "hermes:openrouter:gpt-5",
      runtimeMode: "auto-accept-edits",
      text: "inspect this",
      attachments: [
        {
          id: "image-1",
          name: "screen.png",
          mimeType: "image/png",
          kind: "image",
          size: 4,
          data: "AAAA",
        },
      ],
      onEvent: (event) => events.push(event),
    });

    onStderr?.(
      "2026-09-17 10:24:42 [WARNING] agent.credential_pool: Copilot token exchange degraded to RAW token (exchange unavailable)",
    );

    await initialize();
    await newSession();
    await waitFor(
      () => parse().some((message) => message.method === "session/set_model"),
      "session/set_model",
    );
    const setModel = parse().find(
      (message) => message.method === "session/set_model",
    )!;
    expect(setModel.params.modelId).toBe("openrouter:gpt-5");
    reply(setModel.id, {});

    await waitFor(
      () => parse().some((message) => message.method === "session/set_mode"),
      "session/set_mode",
    );
    const setMode = parse().find(
      (message) => message.method === "session/set_mode",
    )!;
    expect(setMode.params.modeId).toBe("accept_edits");
    reply(setMode.id, {});

    await waitFor(
      () => parse().some((message) => message.method === "session/prompt"),
      "session/prompt",
    );
    const prompt = parse().find(
      (message) => message.method === "session/prompt",
    )!;
    expect(prompt.params.prompt).toEqual([
      { type: "text", text: "inspect this" },
      { type: "image", mimeType: "image/png", data: "AAAA" },
    ]);
    onLine!(JSON.stringify({
      jsonrpc: "2.0",
      method: "session/update",
      params: {
        sessionId: "hermes-session-1",
        update: {
          sessionUpdate: "agent_message_chunk",
          content: { type: "text", text: "Hello" },
        },
      },
    }));
    onLine!(JSON.stringify({
      jsonrpc: "2.0",
      method: "session/update",
      params: {
        sessionId: "hermes-session-1",
        update: {
          sessionUpdate: "agent_thought_chunk",
          content: { type: "text", text: "reasoning" },
        },
      },
    }));
    onLine!(JSON.stringify({
      jsonrpc: "2.0",
      method: "session/update",
      params: {
        sessionId: "hermes-session-1",
        update: {
          sessionUpdate: "tool_call_update",
          toolCallId: "tool-1",
          title: "Read file",
          status: "completed",
        },
      },
    }));
    reply(prompt.id, { stopReason: "end_turn" });

    await turn;
    expect(events).toContainEqual({
      type: "session.providerBound",
      providerSessionId: "hermes-session-1",
    });
    expect(events).toContainEqual({ type: "message.delta", text: "Hello" });
    expect(events).toContainEqual({
      type: "reasoning.delta",
      text: "reasoning",
    });
    expect(events).toContainEqual(
      expect.objectContaining({
        type: "tool.updated",
        callId: "tool-1",
        status: "completed",
      }),
    );
    expect(events.some((event) => event.type === "session.error")).toBe(false);
    await stopHermesSession("hermes-live-new");
  });

  it("loads a bound Hermes session instead of creating a new one", async () => {
    bindHermesSession("hermes-live-load", "persisted-session", "/repo");
    const turn = sendHermesTurn({
      sessionId: "hermes-live-load",
      cwd: "/repo",
      model: "hermes:nous:hermes-4",
      runtimeMode: "supervised",
      text: "continue",
      attachments: [],
      onEvent: () => undefined,
    });

    await initialize();
    await waitFor(
      () => parse().some((message) => message.method === "session/load"),
      "session/load",
    );
    const load = parse().find((message) => message.method === "session/load")!;
    expect(load.params.sessionId).toBe("persisted-session");
    reply(load.id, { models: { currentModelId: "nous:hermes-4" } });

    await waitFor(
      () => parse().some((message) => message.method === "session/set_mode"),
      "session/set_mode",
    );
    const setMode = parse().find(
      (message) => message.method === "session/set_mode",
    )!;
    reply(setMode.id, {});
    await waitFor(
      () => parse().some((message) => message.method === "session/prompt"),
      "session/prompt",
    );
    const prompt = parse().find(
      (message) => message.method === "session/prompt",
    )!;
    reply(prompt.id, { stopReason: "end_turn" });

    await turn;
    expect(parse().some((message) => message.method === "session/new")).toBe(
      false,
    );
    expect(parse().some((message) => message.method === "session/resume")).toBe(
      false,
    );
    await stopHermesSession("hermes-live-load");
  });

  it("uses Hermes permission option ids for supervised approvals", async () => {
    const events: HarnessEvent[] = [];
    const turn = sendHermesTurn({
      sessionId: "hermes-live-permission",
      cwd: "/repo",
      model: "hermes:nous:hermes-4",
      runtimeMode: "supervised",
      text: "run git",
      attachments: [],
      onEvent: (event) => events.push(event),
    });

    await initialize();
    await newSession();
    await waitFor(
      () => parse().some((message) => message.method === "session/set_mode"),
      "session/set_mode",
    );
    const setMode = parse().find(
      (message) => message.method === "session/set_mode",
    )!;
    reply(setMode.id, {});
    await waitFor(
      () => parse().some((message) => message.method === "session/prompt"),
      "session/prompt",
    );
    const prompt = parse().find(
      (message) => message.method === "session/prompt",
    )!;

    onLine!(
      JSON.stringify({
        jsonrpc: "2.0",
        id: 91,
        method: "session/request_permission",
        params: {
          sessionId: "hermes-session-1",
          toolCall: {
            toolCallId: "tool-1",
            title: "Run git status",
            kind: "execute",
            rawInput: { command: "git status" },
          },
          options: [
            { optionId: "allow_once", name: "Allow once", kind: "allow_once" },
            { optionId: "deny", name: "Deny", kind: "reject_once" },
          ],
        },
      }),
    );
    await waitFor(
      () => events.some((event) => event.type === "approval.requested"),
      "approval.requested",
    );
    respondHermesApproval("hermes-live-permission", 91, "allow");
    await waitFor(
      () => parse().some((message) => message.id === 91 && message.result),
      "permission response",
    );
    const response = parse().find(
      (message) => message.id === 91 && message.result,
    )!;
    expect(response.result.outcome.optionId).toBe("allow_once");
    reply(prompt.id, { stopReason: "end_turn" });

    await turn;
    await stopHermesSession("hermes-live-permission");
  });

  it("cancels an in-flight prompt without replaying it", async () => {
    const events: HarnessEvent[] = [];
    const turn = sendHermesTurn({
      sessionId: "hermes-live-cancel",
      cwd: "/repo",
      model: "hermes:nous:hermes-4",
      runtimeMode: "supervised",
      text: "long task",
      attachments: [],
      onEvent: (event) => events.push(event),
    });
    await initialize();
    await newSession();
    await waitFor(
      () => parse().some((message) => message.method === "session/set_mode"),
      "session/set_mode",
    );
    reply(parse().find((message) => message.method === "session/set_mode")!.id, {});
    await waitFor(
      () => parse().some((message) => message.method === "session/prompt"),
      "session/prompt",
    );
    const { cancelHermesTurn } = await import("./hermes");
    blockCancelWrite = true;
    await Promise.race([
      cancelHermesTurn("hermes-live-cancel"),
      new Promise((_, reject) =>
        setTimeout(() => reject(new Error("cancel hung")), 100),
      ),
    ]);
    await turn;
    expect(parse().filter((message) => message.method === "session/prompt")).toHaveLength(1);
    expect(parse().some((message) => message.method === "session/cancel")).toBe(true);
    expect(events.some((event) => event.type === "session.error")).toBe(false);
    await stopHermesSession("hermes-live-cancel");
  });

  it("settles a pending prompt when the ACP child exits", async () => {
    const events: HarnessEvent[] = [];
    const turn = sendHermesTurn({
      sessionId: "hermes-live-exit",
      cwd: "/repo",
      model: "hermes:nous:hermes-4",
      runtimeMode: "supervised",
      text: "crash during turn",
      attachments: [],
      onEvent: (event) => events.push(event),
    });
    await initialize();
    await newSession();
    await waitFor(
      () => parse().some((message) => message.method === "session/set_mode"),
      "session/set_mode",
    );
    reply(parse().find((message) => message.method === "session/set_mode")!.id, {});
    await waitFor(
      () => parse().some((message) => message.method === "session/prompt"),
      "session/prompt",
    );
    onExit!(17);
    await expect(turn).rejects.toThrow("Hermes Agent exited");
    expect(events).toContainEqual({ type: "session.ended", code: 17 });
    await stopHermesSession("hermes-live-exit");
  });
  it("stays busy and resumes after Hermes background subagents finish", async () => {
    const events: HarnessEvent[] = [];
    const transcript = "/tmp/deleg_abcd/task-0.log";
    const manifest = "/tmp/deleg_abcd/manifest.json";
    textFiles.set(
      manifest,
      JSON.stringify({ tasks: [{ index: 0, status: "running" }] }),
    );
    textFiles.set(
      transcript,
      "=== Hermes subagent live transcript ===\n12:00:01 assistant | Found the lifecycle race.\n12:00:02 final | end status=completed",
    );

    const turn = sendHermesTurn({
      sessionId: "hermes-live-background",
      cwd: "/repo",
      model: "hermes:nous:hermes-4",
      runtimeMode: "supervised",
      text: "investigate the race",
      attachments: [],
      onEvent: (event) => events.push(event),
    });
    let settled = false;
    void turn.then(() => {
      settled = true;
    });

    await initialize();
    await newSession();
    await waitFor(
      () => parse().some((message) => message.method === "session/set_mode"),
      "session/set_mode",
    );
    const setMode = parse().find(
      (message) => message.method === "session/set_mode",
    )!;
    reply(setMode.id, {});
    await waitFor(
      () => parse().some((message) => message.method === "session/prompt"),
      "first session/prompt",
    );
    const firstPrompt = parse().find(
      (message) => message.method === "session/prompt",
    )!;

    onLine!(
      JSON.stringify({
        jsonrpc: "2.0",
        method: "session/update",
        params: {
          sessionId: "hermes-session-1",
          update: {
            sessionUpdate: "tool_call_update",
            toolCallId: "delegate-call",
            kind: "agent",
            title: "Delegate task",
            status: "completed",
            content: [
              {
                type: "text",
                text: JSON.stringify({
                  status: "dispatched",
                  mode: "background",
                  delegation_id: "deleg_abcd",
                  live_transcripts: [transcript],
                }),
              },
            ],
          },
        },
      }),
    );
    reply(firstPrompt.id, { stopReason: "end_turn" });

    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(settled).toBe(false);
    expect(
      events.find(
        (event) =>
          event.type === "tool.updated" && event.callId === "delegate-call",
      ),
    ).toMatchObject({ status: "in_progress", kind: "agent" });

    textFiles.set(
      manifest,
      JSON.stringify({
        completed: "2026-09-21 12:00:02",
        tasks: [{ index: 0, status: "completed" }],
      }),
    );
    await waitFor(
      () =>
        parse().filter((message) => message.method === "session/prompt")
          .length === 2,
      "background continuation prompt",
    );
    const continuation = parse().filter(
      (message) => message.method === "session/prompt",
    )[1]!;
    expect(continuation.params.prompt[0].text).toContain(
      "Found the lifecycle race",
    );
    expect(continuation.params.prompt[0].text).toContain(transcript);
    expect(
      events
        .filter(
          (event) =>
            event.type === "tool.updated" && event.callId === "delegate-call",
        )
        .at(-1),
    ).toMatchObject({ status: "completed", kind: "agent" });

    reply(continuation.id, { stopReason: "end_turn" });
    await turn;
    expect(settled).toBe(true);
    await stopHermesSession("hermes-live-background");
  });
});
