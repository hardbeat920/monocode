import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const transport = vi.hoisted(() => ({
  watchers: new Map<string, (line: string) => void>(),
  requests: [] as Array<{
    sessionId: string;
    command: Record<string, unknown>;
  }>,
  prompt: undefined as
    ((sessionId: string, command: Record<string, unknown>) => void) | undefined,
  fast: undefined as
    ((sessionId: string, command: Record<string, unknown>) => void) | undefined,
  writeChild: vi.fn(),
  spawnChild: vi.fn(),
  forkData: undefined as unknown,
  stateSessionId: undefined as string | undefined,
}));

vi.mock("../../core/child", () => ({
  resolveOmpBinary: async () => ({ path: "/fake/omp" }),
  resolvePiBinary: async () => ({ path: "/fake/pi" }),
  acquireHarnessBridge: async () => () => undefined,
  spawnChild: transport.spawnChild,
  killChild: async () => undefined,
  unwatchChild: (id: string) => transport.watchers.delete(id),
  watchChild: (id: string, onLine: (line: string) => void) =>
    transport.watchers.set(id, onLine),
  writeChild: transport.writeChild,
}));

import {
  cancelOmpTurn,
  rewindOmpLastTurn,
  sendOmpTurn,
  steerOmpTurn,
  forgetOmpSession,
} from "./omp";
import { sendPiTurn, steerPiTurn, forgetPiSession } from "../pi/pi";
import { ompCommandProvider, respondQuestion } from "../pi/piFamily";
import { OMP_FLAVOR } from "../pi/piFlavor";
import type { HarnessEvent, SendTurnInput } from "../../core/types";
import { applyHarnessEvent } from "../../core/apply";
import { newSession } from "../../../../features/sessions/model/session";

function frame(sessionId: string, value: Record<string, unknown>) {
  transport.watchers.get(sessionId)?.(JSON.stringify(value));
}
function response(
  sessionId: string,
  command: Record<string, unknown>,
  data?: unknown,
) {
  frame(sessionId, {
    type: "response",
    id: command.id,
    command: command.type,
    success: true,
    data,
  });
}
function input(sessionId = "omp-test", text = "/workflow foo"): SendTurnInput {
  return {
    sessionId,
    text,
    cwd: "/repo",
    model: "omp:default",
    runtimeMode: "supervised",
    onEvent: (event) => events.push(event),
  };
}
let events: HarnessEvent[] = [];

beforeEach(() => {
  events = [];
  transport.requests.length = 0;
  transport.spawnChild.mockReset();
  transport.spawnChild.mockResolvedValue(undefined);
  transport.fast = undefined;
  transport.forkData = undefined;
  transport.stateSessionId = undefined;
  transport.writeChild.mockReset();
  transport.writeChild.mockImplementation(
    async (sessionId: string, line: string) => {
      const command = JSON.parse(line);
      transport.requests.push({ sessionId, command });
      if (command.type === "extension_ui_response") return;
      if (command.type === "prompt")
        return transport.prompt?.(sessionId, command);
      if (command.type === "set_fast_mode" && transport.fast)
        return transport.fast(sessionId, command);
      response(
        sessionId,
        command,
        command.type === "get_state"
          ? { sessionId: transport.stateSessionId ?? "provider-session" }
          : command.type === "get_available_commands"
            ? { commands: [{ name: "workflow", source: "custom" }] }
            : command.type === "get_fork_messages"
              ? { messages: [{ entryId: "entry", text: "latest" }] }
              : command.type === "fork"
                ? (transport.forkData ?? {})
                : {},
      );
    },
  );
});
afterEach(async () => {
  for (const id of [...transport.watchers.keys()]) {
    await forgetOmpSession(id);
    await forgetPiSession(id);
  }
});

async function started(turnInput = input()) {
  let settled = false;
  const turn = sendOmpTurn(turnInput).finally(() => {
    settled = true;
  });
  // Attach a rejection handler before emitting an asynchronous RPC error.
  void turn.catch(() => undefined);
  await vi.waitFor(() =>
    expect(
      transport.requests.some(
        (r) =>
          r.sessionId === turnInput.sessionId && r.command.type === "prompt",
      ),
    ).toBe(true),
  );
  const request = transport.requests
    .filter(
      (r) => r.sessionId === turnInput.sessionId && r.command.type === "prompt",
    )
    .at(-1)!.command;
  return { turn, request, settled: () => settled };
}

describe("Pi-family edit recovery", () => {
  it("reads fork cancellation from the RPC data payload", async () => {
    const running = await started();
    frame("omp-test", {
      type: "prompt_result",
      id: running.request.id,
      agentInvoked: false,
    });
    await running.turn;

    transport.forkData = { cancelled: true };
    await expect(
      rewindOmpLastTurn({
        sessionId: "omp-test",
        cwd: "/repo",
        model: "omp:default",
        runtimeMode: "supervised",
        onEvent: () => undefined,
      }),
    ).rejects.toThrow("Edit cancelled");
  });

  it("rebinds to the provider session created by fork", async () => {
    const running = await started();
    frame("omp-test", {
      type: "prompt_result",
      id: running.request.id,
      agentInvoked: false,
    });
    await running.turn;

    transport.stateSessionId = "forked-provider-session";
    const events: HarnessEvent[] = [];
    await expect(
      rewindOmpLastTurn({
        sessionId: "omp-test",
        cwd: "/repo",
        model: "omp:default",
        runtimeMode: "supervised",
        onEvent: (event) => events.push(event),
      }),
    ).resolves.toEqual({ submitted: false });

    expect(events).toContainEqual({
      type: "session.providerBound",
      providerSessionId: "forked-provider-session",
    });
  });
});

describe("OMP command lifecycle over the real RPC multiplexer", () => {
  it.each([["pi", sendPiTurn], ["omp", sendOmpTurn]] as const)(
    "forwards %s extension result details into the subagent trail",
    async (flavor, send) => {
      const sessionId = `${flavor}-subagent`;
      const turn = send({ ...input(sessionId, "Investigate auth"), model: `${flavor}:default` });
      await vi.waitFor(() => expect(transport.requests.some((r) => r.sessionId === sessionId && r.command.type === "prompt")).toBe(true));
      try {
        frame(sessionId, { type: "tool_execution_start", toolCallId: "spawn", toolName: "task", args: { agent: "scout", task: "Check auth" } });
        const result = { content: [{ type: "text", text: "Found auth" }], details: { results: [
          { agent: "scout", task: "Check auth", exitCode: 0, messages: [{ role: "assistant", content: [{ type: "text", text: "Reading auth" }] }] },
        ] } };
        frame(sessionId, { type: "tool_execution_update", toolCallId: "spawn", partialResult: result });
        frame(sessionId, { type: "tool_execution_end", toolCallId: "spawn", result, isError: false });
        const session = events.reduce(applyHarnessEvent, newSession(flavor, "/repo"));
        const row = session.blocks.find((block) => block.tool?.callId === "spawn");
        expect(row?.agentRun?.steps).toEqual([expect.objectContaining({ kind: "message", text: "Reading auth" })]);
        expect(row?.tool?.status).toBe("completed");
        expect(row?.tool?.detail).toBe("Found auth");
      } finally {
        frame(sessionId, { type: "agent_end" });
        await turn;
      }
    },
  );

  it.each([["pi", sendPiTurn], ["omp", sendOmpTurn]] as const)(
    "keeps a %s tool finished when a progress update arrives after its end",
    async (flavor, send) => {
      const sessionId = `${flavor}-late-update`;
      const turn = send({
        ...input(sessionId, "Run it"),
        model: `${flavor}:default`,
      });
      await vi.waitFor(() =>
        expect(
          transport.requests.some(
            (r) => r.sessionId === sessionId && r.command.type === "prompt",
          ),
        ).toBe(true),
      );
      const text = (value: string) => ({
        content: [{ type: "text", text: value }],
      });
      const update = {
        type: "tool_execution_update",
        toolCallId: "sh",
        partialResult: text("building"),
      };
      try {
        frame(sessionId, {
          type: "tool_execution_start",
          toolCallId: "sh",
          toolName: "bash",
          args: { command: "make" },
        });
        frame(sessionId, update);
        frame(sessionId, {
          type: "tool_execution_end",
          toolCallId: "sh",
          result: text("built"),
          isError: false,
        });
        // omp#12875: steering during bash can deliver an update after the end.
        frame(sessionId, update);
        const row = events
          .reduce(applyHarnessEvent, newSession(flavor, "/repo"))
          .blocks.find((block) => block.tool?.callId === "sh");
        expect(row?.tool).toMatchObject({
          status: "completed",
          detail: "built",
        });
        expect(events).toContainEqual(
          expect.objectContaining({
            type: "tool.updated",
            status: "running",
            detail: "building",
          }),
        );
      } finally {
        frame(sessionId, { type: "agent_end" });
        await turn;
      }
    },
  );

  it.each([
    ["pi", sendPiTurn, steerPiTurn],
    ["omp", sendOmpTurn, steerOmpTurn],
  ] as const)(
    "delivers attachment-only prompts and steering through %s",
    async (flavor, send, steer) => {
      const sessionId = `${flavor}-attachments`;
      const attachments = [
        {
          id: "pdf",
          name: "report.pdf",
          kind: "file" as const,
          mimeType: "application/pdf",
          size: 100,
          path: "/tmp/report.pdf",
        },
      ];
      const turnInput = {
        ...input(sessionId, ""),
        model: `${flavor}:default`,
        attachments,
      };
      const turn = send(turnInput);
      void turn.catch(() => undefined);
      try {
        await vi.waitFor(() =>
          expect(
            transport.requests.some(
              (r) => r.sessionId === sessionId && r.command.type === "prompt",
            ),
          ).toBe(true),
        );
        expect(
          transport.requests.find(
            (r) => r.sessionId === sessionId && r.command.type === "prompt",
          )?.command.message,
        ).toBe('Attached file (read from disk): "/tmp/report.pdf"');
        await steer(turnInput);
        expect(
          transport.requests.find(
            (r) => r.sessionId === sessionId && r.command.type === "steer",
          )?.command.message,
        ).toBe('Attached file (read from disk): "/tmp/report.pdf"');
      } finally {
        frame(sessionId, { type: "agent_end" });
        await turn;
      }
    },
  );

  it("applies fast mode through OMP RPC before prompting", async () => {
    const running = await started({
      ...input(),
      modelSettings: { fast: "true" },
    });
    const fast = transport.requests.find(
      (request) => request.command.type === "set_fast_mode",
    );
    const promptIndex = transport.requests.findIndex(
      (request) => request.command.type === "prompt",
    );

    expect(fast?.command).toMatchObject({
      type: "set_fast_mode",
      enabled: true,
    });
    expect(transport.requests.indexOf(fast!)).toBeLessThan(promptIndex);
    frame("omp-test", { type: "agent_end" });
    await running.turn;
  });

  it("keeps fast mode in sync with OMP config updates", async () => {
    const running = await started();
    frame("omp-test", {
      type: "config_update",
      fastModeEnabled: true,
    });

    expect(events).toContainEqual({
      type: "session.configChanged",
      modelSettings: { fast: "true" },
    });
    frame("omp-test", { type: "agent_end" });
    await running.turn;
  });

  it("falls back cleanly when the current model cannot use fast mode", async () => {
    transport.fast = (sessionId, command) => {
      frame(sessionId, {
        type: "response",
        id: command.id,
        command: command.type,
        success: false,
        error: "Fast mode is unavailable for the current model.",
      });
    };
    const running = await started({
      ...input(),
      modelSettings: { fast: "true" },
    });

    expect(
      transport.requests.filter(
        (request) => request.command.type === "set_fast_mode",
      ),
    ).toHaveLength(1);
    expect(events).toContainEqual({
      type: "session.configChanged",
      modelSettings: { fast: "false" },
    });
    expect(events).toContainEqual({
      type: "status",
      text: "Fast mode is unavailable for the current model.",
    });
    frame("omp-test", { type: "agent_end" });
    await running.turn;
  });

  it("reflects command-driven model/settings and session changes in MonoCode", async () => {
    const running = await started();
    frame("omp-test", {
      type: "config_update",
      model: { provider: "anthropic", id: "new-model" },
      thinkingLevel: "high",
    });
    frame("omp-test", {
      type: "session_info_update",
      sessionId: "new-provider-session",
      title: "New session",
    });
    const config = events.find((e) => e.type === "session.configChanged")!;
    expect(config).toEqual({
      type: "session.configChanged",
      model: "omp:anthropic/new-model",
      modelSettings: { thinking: "high" },
    });
    expect(events).toContainEqual({
      type: "session.providerBound",
      providerSessionId: "new-provider-session",
    });
    const session = {
      ...newSession("omp"),
      modelSettings: { existing: "keep" },
    };
    const updated = applyHarnessEvent(session, config);
    expect(updated.model).toBe("omp:anthropic/new-model");
    expect(updated.modelSettings).toEqual({
      existing: "keep",
      thinking: "high",
    });
    frame("omp-test", { type: "agent_end" });
    await running.turn;
  });
  it("sends arguments unchanged and renders local output without waiting for agent_end", async () => {
    transport.prompt = (id, command) => {
      frame(id, {
        type: "command_output",
        text: "\u001b[32mSelected reviewer: careful\u001b[0m",
      });
      response(id, command, { agentInvoked: false });
    };
    await sendOmpTurn(input());
    expect(
      transport.requests.find((r) => r.command.type === "prompt")?.command
        .message,
    ).toBe("/workflow foo");
    expect(events).toContainEqual({
      type: "status",
      text: "Selected reviewer: careful",
    });
    expect(events.filter((e) => e.type === "message.completed")).toHaveLength(
      1,
    );
    expect(transport.spawnChild).toHaveBeenCalledWith(
      "omp-test",
      "/fake/omp",
      ["--mode", "rpc"],
      "/repo",
    );
  });

  it("reports when OMP accepts a turn", async () => {
    const onAccepted = vi.fn();
    transport.prompt = (id, command) => {
      response(id, command, { agentInvoked: false });
    };

    await sendOmpTurn({ ...input(), onAccepted });

    expect(onAccepted).toHaveBeenCalledOnce();
  });

  it.each(["before", "after"])(
    "handles prompt_result %s its acknowledgement",
    async (order) => {
      transport.prompt = (id, command) => {
        if (order === "before")
          frame(id, {
            type: "prompt_result",
            id: command.id,
            agentInvoked: false,
          });
        response(id, command);
      };
      const running = await started();
      if (order === "after")
        frame("omp-test", {
          type: "prompt_result",
          id: running.request.id,
          agentInvoked: false,
        });
      await running.turn;
      expect(events.filter((e) => e.type === "message.completed")).toHaveLength(
        1,
      );
    },
  );

  it("ignores results belonging to another request and nonterminal agent_end", async () => {
    const running = await started();
    frame("omp-test", {
      type: "prompt_result",
      id: "another-request",
      agentInvoked: false,
    });
    frame("omp-test", {
      type: "prompt_result",
      id: running.request.id,
      agentInvoked: true,
    });
    frame("omp-test", { type: "agent_end", isTerminal: false });
    await Promise.resolve();
    expect(running.settled()).toBe(false);
    expect(events.some((e) => e.type === "message.completed")).toBe(false);
    frame("omp-test", { type: "agent_end", isTerminal: true });
    await running.turn;
  });

  it("keeps normal chat and agent-invoking commands active until terminal completion", async () => {
    transport.prompt = (id, command) =>
      response(id, command, { agentInvoked: true });
    const running = await started(input("omp-test", "Explain this code"));
    expect(running.request.message).toBe("Explain this code");
    expect(running.settled()).toBe(false);
    frame("omp-test", { type: "agent_end" });
    await running.turn;
  });

  it("reports prompt errors delivered after the acknowledgement", async () => {
    const running = await started();
    frame("omp-test", {
      type: "response",
      command: "prompt",
      id: running.request.id,
      success: false,
      error: "Workflow failed",
    });
    await expect(running.turn).rejects.toThrow("Workflow failed");
    expect(events).toContainEqual({
      type: "session.error",
      message: "Workflow failed",
    });
  });

  it("cancels an unacknowledged command and ignores its late completion during the next turn", async () => {
    transport.prompt = () => undefined;
    const first = await started();
    await cancelOmpTurn("omp-test");
    await first.turn;
    transport.requests.length = 0;
    transport.prompt = (id, command) => response(id, command);
    const second = await started(input("omp-test", "hello"));
    frame("omp-test", {
      type: "prompt_result",
      id: first.request.id,
      agentInvoked: false,
    });
    frame("omp-test", {
      type: "response",
      command: "prompt",
      id: first.request.id,
      success: true,
      data: { agentInvoked: false },
    });
    expect(second.settled()).toBe(false);
    frame("omp-test", { type: "agent_end" });
    await second.turn;
  });

  it("uses prompt for a slash command submitted while streaming, without finishing the main turn", async () => {
    const running = await started(input("omp-test", "hello"));
    transport.prompt = (id, command) =>
      response(id, command, { agentInvoked: false });
    await steerOmpTurn({ ...input(), text: "/usage" });
    expect(transport.requests.at(-1)?.command).toMatchObject({
      type: "prompt",
      message: "/usage",
      streamingBehavior: "steer",
    });
    expect(running.settled()).toBe(false);
    frame("omp-test", { type: "agent_end" });
    await running.turn;
  });

  it("keeps Pi's normal retry and completion path intact", async () => {
    let settled = false;
    const turn = sendPiTurn({
      ...input("pi-test", "/skill:architect foo"),
      model: "pi:default",
    }).then(() => {
      settled = true;
    });
    await vi.waitFor(() =>
      expect(transport.requests.some((r) => r.command.type === "prompt")).toBe(
        true,
      ),
    );
    frame("pi-test", { type: "agent_end", willRetry: true });
    await Promise.resolve();
    expect(settled).toBe(false);
    frame("pi-test", { type: "agent_settled" });
    await turn;
    expect(events.filter((e) => e.type === "message.completed")).toHaveLength(
      1,
    );
  });

  it("surfaces displayed OMP advisor notes and hides internal messages", async () => {
    const running = await started();
    const advisor = {
      role: "custom",
      customType: "advisor",
      display: true,
      content: "raw advisory envelope",
      details: {
        notes: [
          { note: "Minor note", severity: "nit" },
          { note: "Stop here", severity: "blocker" },
        ],
      },
    };
    frame("omp-test", { type: "message_start", message: advisor });
    frame("omp-test", { type: "message_end", message: advisor });
    frame("omp-test", {
      type: "message_start",
      message: {
        role: "custom",
        customType: "xdev-mount-notice",
        display: false,
        content: "internal mount details",
      },
    });
    frame("omp-test", { type: "advisor_yielded" });

    expect(events).toContainEqual({
      type: "interjection",
      text: "Minor note\n\nStop here",
      customType: "advisor",
      severity: "blocker",
    });
    expect(
      events.filter((event) => event.type === "interjection"),
    ).toHaveLength(1);
    expect(
      events.some(
        (event) => "text" in event && event.text === "internal mount details",
      ),
    ).toBe(false);
    frame("omp-test", { type: "agent_end" });
    await running.turn;
  });

  it("emits a transient status when the OMP advisor yields", async () => {
    const running = await started();
    const start = events.length;
    frame("omp-test", { type: "advisor_yielded" });
    expect(events.slice(start)).toEqual([
      { type: "status", text: expect.any(String) },
    ]);
    frame("omp-test", { type: "agent_end" });
    await running.turn;
  });

  it("uses generic content for other displayed OMP custom messages", async () => {
    const running = await started();
    frame("omp-test", {
      type: "message_start",
      message: {
        role: "custom",
        customType: "extension-notice",
        display: true,
        content: [
          { type: "text", text: "Extension changed the plan." },
          { type: "image", data: "ignored", mimeType: "image/png" },
          { type: "text", text: "Review it." },
        ],
      },
    });

    expect(events).toContainEqual({
      type: "interjection",
      text: "Extension changed the plan.\nReview it.",
      customType: "extension-notice",
    });
    frame("omp-test", { type: "agent_end" });
    await running.turn;
  });

  it("does not reinterpret Pi custom messages as OMP interjections", async () => {
    const turn = sendPiTurn({
      ...input("pi-test", "hello"),
      model: "pi:default",
    });
    await vi.waitFor(() =>
      expect(transport.requests.some((r) => r.command.type === "prompt")).toBe(
        true,
      ),
    );
    frame("pi-test", {
      type: "message_start",
      message: {
        role: "custom",
        customType: "advisor",
        display: true,
        content: "Pi-owned custom frame",
      },
    });
    expect(events.some((event) => event.type === "interjection")).toBe(false);
    frame("pi-test", { type: "agent_end" });
    await turn;
  });
});

describe("OMP live command inventories", () => {
  it("loads through the active process and isolates push updates by session and directory", async () => {
    const a: unknown[] = [],
      b: unknown[] = [],
      otherCwd: unknown[] = [];
    const unsub = [
      ompCommandProvider.subscribe!(
        { sessionId: "omp-test", cwd: "/repo/" },
        (commands) => a.push(commands),
      ),
      ompCommandProvider.subscribe!(
        { sessionId: "other", cwd: "/repo" },
        (commands) => b.push(commands),
      ),
      ompCommandProvider.subscribe!(
        { sessionId: "omp-test", cwd: "/elsewhere" },
        (commands) => otherCwd.push(commands),
      ),
    ];
    const running = await started();
    await expect(
      ompCommandProvider.discover({ sessionId: "omp-test", cwd: "/repo" }),
    ).resolves.toMatchObject([{ name: "workflow" }]);
    frame("omp-test", {
      type: "available_commands_update",
      commands: [{ name: "review", source: "extension" }],
    });
    frame("omp-test", { type: "available_commands_update", commands: null });
    expect(a).toHaveLength(1);
    expect(b).toHaveLength(0);
    expect(otherCwd).toHaveLength(0);
    await expect(
      ompCommandProvider.discover({ sessionId: "omp-test", cwd: "/repo" }),
    ).resolves.toMatchObject([{ name: "review" }]);
    for (const unsubscribe of unsub) unsubscribe();
    frame("omp-test", { type: "available_commands_update", commands: [] });
    expect(a).toHaveLength(1);
    frame("omp-test", { type: "agent_end" });
    await running.turn;
  });
});

describe("OMP workflow dialogs", () => {
  it("returns the actual selected option with its original text", async () => {
    const running = await started();
    frame("omp-test", {
      type: "extension_ui_request",
      id: "choose",
      method: "select",
      title: "Choose reviewer",
      options: ["fast", "\u001b[32mcareful\u001b[0m"],
    });
    const question = events.find((e) => e.type === "question.asked");
    expect(question?.questions[0]?.options.map((o) => o.label)).toEqual([
      "fast",
      "careful",
    ]);
    respondQuestion(OMP_FLAVOR, "omp-test", question!.requestId, {
      kind: "answered",
      answers: { choose: ["1"] },
    });
    await vi.waitFor(() =>
      expect(transport.requests.at(-1)?.command).toEqual({
        type: "extension_ui_response",
        id: "choose",
        value: "\u001b[32mcareful\u001b[0m",
      }),
    );
    frame("omp-test", {
      type: "prompt_result",
      id: running.request.id,
      agentInvoked: false,
    });
    await running.turn;
  });

  it.each(["input", "editor"])(
    "returns %s text to the workflow",
    async (method) => {
      const running = await started();
      frame("omp-test", {
        type: "extension_ui_request",
        id: "text",
        method,
        title: "Instructions",
      });
      const question = events.find((e) => e.type === "question.asked");
      respondQuestion(OMP_FLAVOR, "omp-test", question!.requestId, {
        kind: "answered",
        answers: {},
        custom: { text: "Review security\nand correctness" },
      });
      await vi.waitFor(() =>
        expect(transport.requests.at(-1)?.command).toMatchObject({
          type: "extension_ui_response",
          value: "Review security\nand correctness",
        }),
      );
      frame("omp-test", {
        type: "prompt_result",
        id: running.request.id,
        agentInvoked: false,
      });
      await running.turn;
    },
  );

  it("resolves pending questions when the user stops a command", async () => {
    const running = await started();
    frame("omp-test", {
      type: "extension_ui_request",
      id: "text",
      method: "input",
      title: "Instructions",
    });
    await cancelOmpTurn("omp-test");
    await running.turn;
    expect(transport.requests.map((r) => r.command)).toContainEqual({
      type: "extension_ui_response",
      id: "text",
      cancelled: true,
    });
    expect(events.some((e) => e.type === "question.resolved")).toBe(true);
  });
});
