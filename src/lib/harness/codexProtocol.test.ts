import { describe, expect, it } from "vitest";
import {
  applyCodexTextUpdate,
  buildThreadStartParams,
  buildTurnStartParams,
  buildTurnSteerParams,
  createCodexTextState,
  invalidateCodexTextTail,
  isRecoverableThreadResumeError,
  mapApprovalRequest,
  mapCodexNotification,
  runtimeModeToCodexConfig,
  toCodexApprovalDecision,
} from "./codexProtocol";
import { parseCodexModelList } from "./codexCatalog";

describe("runtimeModeToCodexConfig", () => {
  it("maps supervised to untrusted read-only", () => {
    expect(runtimeModeToCodexConfig("supervised")).toEqual({
      approvalPolicy: "untrusted",
      sandbox: "read-only",
      approvalsReviewer: "user",
      sandboxPolicy: { type: "readOnly" },
    });
  });

  it("maps auto-accept-edits to workspace-write with user reviewer", () => {
    expect(runtimeModeToCodexConfig("auto-accept-edits")).toMatchObject({
      approvalPolicy: "on-request",
      sandbox: "workspace-write",
      approvalsReviewer: "user",
      sandboxPolicy: { type: "workspaceWrite" },
    });
  });

  it("maps auto to workspace-write with auto_review", () => {
    expect(runtimeModeToCodexConfig("auto")).toMatchObject({
      approvalPolicy: "on-request",
      approvalsReviewer: "auto_review",
      sandboxPolicy: { type: "workspaceWrite" },
    });
  });

  it("maps full-access to never + danger-full-access", () => {
    expect(runtimeModeToCodexConfig("full-access")).toMatchObject({
      approvalPolicy: "never",
      sandbox: "danger-full-access",
      sandboxPolicy: { type: "dangerFullAccess" },
    });
  });
});

describe("buildThreadStartParams / buildTurnStartParams", () => {
  it("includes model and omits default service tier", () => {
    const thread = buildThreadStartParams({
      cwd: "/tmp/proj",
      runtimeMode: "supervised",
      model: "gpt-5.4",
      serviceTier: "default",
    });
    expect(thread).toMatchObject({
      cwd: "/tmp/proj",
      model: "gpt-5.4",
      approvalPolicy: "untrusted",
    });
    expect(thread.serviceTier).toBeUndefined();
  });

  it("builds turn input with text and image attachments", () => {
    const turn = buildTurnStartParams({
      threadId: "thr_1",
      runtimeMode: "auto-accept-edits",
      prompt: "hello",
      attachments: [{ type: "image", url: "data:image/png;base64,abc" }],
      model: "gpt-5.4",
      effort: "high",
      serviceTier: "fast",
    });
    expect(turn.threadId).toBe("thr_1");
    expect(turn.effort).toBe("high");
    expect(turn.serviceTier).toBe("fast");
    expect(turn.input).toEqual([
      { type: "text", text: "hello" },
      { type: "image", url: "data:image/png;base64,abc" },
    ]);
    expect(turn.sandboxPolicy).toEqual({ type: "workspaceWrite" });
  });

  it("builds steer input with expected turn id", () => {
    const steer = buildTurnSteerParams({
      threadId: "thr_1",
      expectedTurnId: "turn_9",
      prompt: "focus on tests",
    });
    expect(steer).toEqual({
      threadId: "thr_1",
      expectedTurnId: "turn_9",
      input: [{ type: "text", text: "focus on tests" }],
    });
  });
});

describe("isRecoverableThreadResumeError", () => {
  it("detects missing-thread errors", () => {
    expect(
      isRecoverableThreadResumeError(new Error("Thread thr_x not found")),
    ).toBe(true);
    expect(isRecoverableThreadResumeError(new Error("unknown thread id"))).toBe(
      true,
    );
  });

  it("rejects unrelated errors", () => {
    expect(isRecoverableThreadResumeError(new Error("rate limited"))).toBe(
      false,
    );
    expect(isRecoverableThreadResumeError(new Error("network down"))).toBe(
      false,
    );
  });
});

describe("mapCodexNotification", () => {
  it("maps agent message deltas", () => {
    const mapped = mapCodexNotification("item/agentMessage/delta", {
      itemId: "msg_1",
      delta: "Hello",
    });
    expect(mapped.textUpdates).toEqual([
      {
        kind: "delta",
        itemId: "msg_1",
        channel: "assistant",
        text: "Hello",
      },
    ]);
    expect(mapped.events).toEqual([]);
  });

  it("preserves whitespace-only identified deltas", () => {
    const mapped = mapCodexNotification("item/agentMessage/delta", {
      itemId: "msg_1",
      delta: "\n\n",
    });
    expect(mapped.textUpdates).toEqual([
      {
        kind: "delta",
        itemId: "msg_1",
        channel: "assistant",
        text: "\n\n",
      },
    ]);
  });

  it("maps reasoning summary deltas", () => {
    const mapped = mapCodexNotification("item/reasoning/summaryTextDelta", {
      itemId: "reason_1",
      delta: "thinking…",
    });
    expect(mapped.textUpdates).toEqual([
      {
        kind: "delta",
        itemId: "reason_1",
        channel: "reasoning-summary",
        text: "thinking…",
      },
    ]);
  });

  it("retains completed agent message identity", () => {
    const mapped = mapCodexNotification("item/completed", {
      item: { id: "msg_1", type: "agentMessage", text: "Hello" },
    });
    expect(mapped.textUpdates).toEqual([
      {
        kind: "completed",
        itemId: "msg_1",
        channel: "assistant",
        text: "Hello",
      },
    ]);
    expect(mapped.events).toEqual([{ type: "message.completed" }]);
    expect(mapped.scheduleTurnWatchdog).toBe(true);
  });

  it("rejects anonymous text deltas", () => {
    expect(
      mapCodexNotification("item/agentMessage/delta", { delta: "Hello" }),
    ).toEqual({ events: [] });
  });

  it("ignores userMessage items echoed by Codex", () => {
    const started = mapCodexNotification("item/started", {
      item: {
        id: "msg_1",
        type: "userMessage",
        content: [{ type: "text", text: "hey are you there" }],
      },
    });
    expect(started.events).toEqual([]);

    const completed = mapCodexNotification("item/completed", {
      item: {
        id: "msg_1",
        type: "userMessage",
        content: [{ type: "text", text: "hey are you there" }],
      },
    });
    expect(completed.events).toEqual([]);
  });

  it("maps command execution item lifecycle", () => {
    const started = mapCodexNotification("item/started", {
      item: {
        id: "cmd_1",
        type: "commandExecution",
        command: "ls -la",
        status: "inProgress",
      },
    });
    expect(started.events[0]).toMatchObject({
      type: "tool.started",
      callId: "cmd_1",
      title: "ls -la",
      kind: "execute",
    });

    const completed = mapCodexNotification("item/completed", {
      item: {
        id: "cmd_1",
        type: "commandExecution",
        command: "ls -la",
        status: "completed",
        aggregatedOutput: "ok",
      },
    });
    expect(completed.events[0]).toMatchObject({
      type: "tool.updated",
      callId: "cmd_1",
      status: "completed",
      detail: "ok",
    });
  });

  it("maps file change items", () => {
    const mapped = mapCodexNotification("item/started", {
      item: {
        id: "fc_1",
        type: "fileChange",
        status: "inProgress",
        changes: [
          {
            path: "src/App.tsx",
            kind: "update",
            diff: "@@ -1 +1 @@\n-old\n+new\n",
          },
        ],
      },
    });
    expect(mapped.events[0]).toMatchObject({
      type: "tool.started",
      callId: "fc_1",
      kind: "edit",
    });
  });

  it("maps turn completion and clears active turn", () => {
    const mapped = mapCodexNotification("turn/completed", {
      turn: { id: "turn_1", status: "completed" },
    });
    expect(mapped.turnCompleted?.status).toBe("completed");
    expect(mapped.activeTurnId).toBeNull();
    expect(mapped.events).toEqual(
      expect.arrayContaining([
        { type: "message.completed" },
        { type: "reasoning.completed" },
      ]),
    );
  });

  it("maps aborted turns as interrupted completion", () => {
    const mapped = mapCodexNotification("turn/aborted", {
      turn: { id: "turn_1" },
    });
    expect(mapped.turnCompleted?.status).toBe("interrupted");
    expect(mapped.activeTurnId).toBeNull();
  });

  it("maps failed turns to session.error", () => {
    const mapped = mapCodexNotification("turn/completed", {
      turn: {
        id: "turn_1",
        status: "failed",
        error: { message: "quota exceeded" },
      },
    });
    expect(mapped.turnCompleted?.status).toBe("failed");
    expect(mapped.events).toContainEqual({
      type: "session.error",
      message: "quota exceeded",
    });
  });

  it("ignores unknown methods", () => {
    expect(mapCodexNotification("future/unknown", { x: 1 }).events).toEqual([]);
  });
});

describe("Codex text state", () => {
  it("deduplicates an equal completed value", () => {
    let state = createCodexTextState();
    state = applyCodexTextUpdate(state, {
      kind: "started",
      itemId: "msg_1",
      channel: "assistant",
    }).state;
    const delta = applyCodexTextUpdate(state, {
      kind: "delta",
      itemId: "msg_1",
      channel: "assistant",
      text: "Hello",
    });
    expect(delta.event).toEqual({ type: "message.delta", text: "Hello" });

    const completed = applyCodexTextUpdate(delta.state, {
      kind: "completed",
      itemId: "msg_1",
      channel: "assistant",
      text: "Hello",
    });
    expect(completed.event).toBeUndefined();
    expect(completed.conflict).toBeUndefined();
  });

  it("emits a completed-only fallback while the item is at the tail", () => {
    const started = applyCodexTextUpdate(createCodexTextState(), {
      kind: "started",
      itemId: "msg_1",
      channel: "assistant",
    });
    const completed = applyCodexTextUpdate(started.state, {
      kind: "completed",
      itemId: "msg_1",
      channel: "assistant",
      text: "Hello",
    });
    expect(completed.event).toEqual({ type: "message.delta", text: "Hello" });
  });

  it("rejects a completion after another transcript operation", () => {
    const started = applyCodexTextUpdate(createCodexTextState(), {
      kind: "started",
      itemId: "msg_1",
      channel: "assistant",
    });
    const completed = applyCodexTextUpdate(
      invalidateCodexTextTail(started.state),
      {
        kind: "completed",
        itemId: "msg_1",
        channel: "assistant",
        text: "Hello",
      },
    );
    expect(completed.event).toBeUndefined();
    expect(completed.conflict).toBe("position");
  });

  it("rejects divergent completed text", () => {
    const delta = applyCodexTextUpdate(createCodexTextState(), {
      kind: "delta",
      itemId: "msg_1",
      channel: "assistant",
      text: "help",
    });
    const completed = applyCodexTextUpdate(delta.state, {
      kind: "completed",
      itemId: "msg_1",
      channel: "assistant",
      text: "hello",
    });
    expect(completed.event).toBeUndefined();
    expect(completed.conflict).toBe("text");
  });
});

describe("approvals", () => {
  it("maps command approval requests", () => {
    const mapped = mapApprovalRequest(
      "item/commandExecution/requestApproval",
      { itemId: "cmd_1", command: "rm -rf /", reason: "cleanup" },
      7,
    );
    expect(mapped).toMatchObject({
      kind: "command",
      event: {
        type: "approval.requested",
        requestId: 7,
        callId: "cmd_1",
        kind: "execute",
      },
    });
  });

  it("maps file-change approval requests", () => {
    const mapped = mapApprovalRequest(
      "item/fileChange/requestApproval",
      { itemId: "fc_1", reason: "Write config" },
      3,
    );
    expect(mapped?.kind).toBe("file-change");
    expect(mapped?.event.title).toBe("Write config");
  });

  it("translates UI decisions to Codex wire decisions", () => {
    expect(toCodexApprovalDecision("allow", "command")).toBe("accept");
    expect(toCodexApprovalDecision("deny", "file-change")).toBe("decline");
  });
});

describe("parseCodexModelList", () => {
  it("builds models with reasoning and service tier settings", () => {
    const models = parseCodexModelList([
      {
        model: "gpt-5.6-luna",
        displayName: "gpt-5.6-luna",
        defaultReasoningEffort: "medium",
        supportedReasoningEfforts: [
          { reasoningEffort: "low" },
          { reasoningEffort: "medium" },
          { reasoningEffort: "high" },
        ],
        serviceTiers: [{ id: "fast", name: "Fast" }],
        defaultServiceTier: "default",
      },
      {
        model: "gpt-5.6-terra",
        displayName: "gpt-5.6-terra",
        isDefault: true,
        supportedReasoningEfforts: [],
      },
    ]);
    expect(models.map((m) => m.nativeId)).toEqual([
      "gpt-5.6-terra",
      "gpt-5.6-luna",
    ]);
    const luna = models.find((m) => m.nativeId === "gpt-5.6-luna");
    expect(luna?.settings?.some((s) => s.id === "reasoningEffort")).toBe(true);
    expect(luna?.settings?.some((s) => s.id === "serviceTier")).toBe(true);
    expect(luna?.settings?.find((s) => s.id === "reasoningEffort")?.value).toBe(
      "medium",
    );
  });
});

describe("mapCodexNotification thread/tokenUsage/updated", () => {
  it("reports the last request and the window the app-server supplies", () => {
    const mapped = mapCodexNotification("thread/tokenUsage/updated", {
      threadId: "t1",
      turnId: "turn1",
      tokenUsage: {
        last: {
          totalTokens: 42_000,
          inputTokens: 40_000,
          cachedInputTokens: 30_000,
          cacheWriteInputTokens: 0,
          outputTokens: 2_000,
          reasoningOutputTokens: 500,
        },
        total: {
          totalTokens: 900_000,
          inputTokens: 880_000,
          cachedInputTokens: 800_000,
          cacheWriteInputTokens: 0,
          outputTokens: 20_000,
          reasoningOutputTokens: 4_000,
        },
        modelContextWindow: 272_000,
      },
    });
    expect(mapped.events).toEqual([
      { type: "context", used: 42_000, window: 272_000 },
    ]);
  });

  it("never uses `total`, which keeps climbing past the window", () => {
    const mapped = mapCodexNotification("thread/tokenUsage/updated", {
      tokenUsage: {
        last: { totalTokens: 10_000 },
        total: { totalTokens: 5_000_000 },
        modelContextWindow: 272_000,
      },
    });
    expect(mapped.events).toEqual([
      { type: "context", used: 10_000, window: 272_000 },
    ]);
  });

  it("omits the window when the app-server does not know it", () => {
    const mapped = mapCodexNotification("thread/tokenUsage/updated", {
      tokenUsage: {
        last: { totalTokens: 10_000 },
        total: { totalTokens: 10_000 },
        modelContextWindow: null,
      },
    });
    expect(mapped.events).toEqual([{ type: "context", used: 10_000 }]);
  });

  it("stays quiet on an empty reading", () => {
    expect(
      mapCodexNotification("thread/tokenUsage/updated", {
        tokenUsage: { last: {}, total: {} },
      }).events,
    ).toEqual([]);
  });
});
