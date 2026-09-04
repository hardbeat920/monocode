import { nativeModelId } from "../models";
import type { Attachment, RuntimeMode } from "../session";
import {
  killChild,
  resolveCodexBinary,
  spawnChild,
  unwatchChild,
  watchChild,
} from "./child";
import {
  asRecord,
  buildThreadStartParams,
  buildTurnStartParams,
  buildTurnSteerParams,
  isRecoverableThreadResumeError,
  mapApprovalRequest,
  mapCodexNotification,
  toCodexApprovalDecision,
  type CodexApprovalKind,
} from "./codexProtocol";
import { JsonRpcClient, type JsonRpcId } from "./jsonRpc";
import { joinStreamText, snapshotRemainder } from "./streamText";
import type {
  ApprovalDecision,
  CompactContextInput,
  HarnessEvent,
  HarnessSessionInput,
  SendTurnInput,
  SteerTurnInput,
} from "./types";

type PendingApproval = {
  rpcId: JsonRpcId;
  kind: CodexApprovalKind;
  resolve: (decision: ApprovalDecision) => void;
};

type Live = {
  rpc: JsonRpcClient;
  threadId: string;
  cwd: string;
  runtimeMode: RuntimeMode;
  planning: boolean;
  onEvent: (event: HarnessEvent) => void;
  approvals: Map<number, PendingApproval>;
  nextApprovalUiId: number;
  cancelled: boolean;
  muteUpdates: boolean;
  activeTurnId: string | null;
  turns: Promise<void>;
  /** Resolves when the current turn completes (or is cancelled). */
  turnDone: (() => void) | null;
  turnFailed: ((error: Error) => void) | null;
  /** turn/completed arrived before runTurn registered turnDone. */
  turnEndPending: boolean;
  emittedAssistant: string;
  emittedReasoning: string;
};

type Resume = {
  threadId: string;
  cwd: string;
};

const liveByThread = new Map<string, Live>();
const resumeByThread = new Map<string, Resume>();
const cancelledThreads = new Set<string>();

let resolveCodexBinaryImpl: () => Promise<{ path: string }> =
  resolveCodexBinary;

/** Test seam. */
export function setCodexBinaryResolver(
  fn: () => Promise<{ path: string }>,
): void {
  resolveCodexBinaryImpl = fn;
}

export async function sendCodexTurn(input: SendTurnInput): Promise<void> {
  let live: Live;
  try {
    live = await ensureLive(input);
  } catch (error) {
    cancelledThreads.delete(input.sessionId);
    throw error;
  }
  if (cancelledThreads.delete(input.sessionId)) return;

  live.onEvent = input.onEvent;
  live.runtimeMode = input.runtimeMode;
  live.planning = input.intent === "plan";
  live.turns = live.turns
    .catch(() => undefined)
    .then(async () => {
      live.cancelled = false;
      live.muteUpdates = false;
      try {
        await runTurn(live, input);
      } catch (error) {
        if (live.cancelled) return;
        throw error;
      }
    });
  await live.turns;
}

export async function compactCodexContext(
  input: CompactContextInput,
): Promise<void> {
  let live: Live;
  try {
    live = await ensureLive(input);
  } catch (error) {
    cancelledThreads.delete(input.sessionId);
    throw error;
  }
  if (cancelledThreads.delete(input.sessionId)) return;

  live.onEvent = input.onEvent;
  live.turns = live.turns
    .catch(() => undefined)
    .then(async () => {
      live.cancelled = false;
      live.muteUpdates = false;
      try {
        await runCompaction(live);
      } catch (error) {
        if (live.cancelled) return;
        throw error;
      }
    });
  await live.turns;
}

export async function steerCodexTurn(input: SteerTurnInput): Promise<void> {
  const live = liveByThread.get(input.sessionId);
  if (!live) throw new Error("No active Codex session");
  const turnId = live.activeTurnId;
  if (!turnId) throw new Error("No active turn to steer");

  const attachments = await codexAttachments(input.attachments ?? []);
  const params = buildTurnSteerParams({
    threadId: live.threadId,
    expectedTurnId: turnId,
    prompt: input.text.trim() || undefined,
    attachments,
  });
  if (
    !params.input ||
    (Array.isArray(params.input) && params.input.length === 0)
  ) {
    return;
  }

  await live.rpc.request("turn/steer", params);
}

export function respondCodexApproval(
  sessionId: string,
  requestId: number,
  decision: ApprovalDecision,
): void {
  const live = liveByThread.get(sessionId);
  const pending = live?.approvals.get(requestId);
  if (!pending) return;
  pending.resolve(decision);
}

export async function cancelCodexTurn(sessionId: string): Promise<void> {
  const live = liveByThread.get(sessionId);
  if (!live) {
    cancelledThreads.add(sessionId);
    return;
  }
  live.cancelled = true;
  live.muteUpdates = true;
  for (const [, pending] of live.approvals) {
    pending.resolve("deny");
  }
  live.approvals.clear();
  const turnId = live.activeTurnId;
  if (turnId) {
    await live.rpc
      .request("turn/interrupt", {
        threadId: live.threadId,
        turnId,
      })
      .catch(() => undefined);
  }
  finishActiveTurn(live, [
    { type: "message.completed" },
    { type: "reasoning.completed" },
  ]);
}

export async function stopCodexSession(sessionId: string): Promise<void> {
  cancelledThreads.delete(sessionId);
  const live = liveByThread.get(sessionId);
  liveByThread.delete(sessionId);
  if (live) {
    live.muteUpdates = true;
    live.turnDone?.();
    live.turnDone = null;
    live.turnFailed = null;
    live.rpc.close();
  }
  unwatchChild(sessionId);
  await killChild(sessionId).catch(() => undefined);
}

export async function forgetCodexSession(sessionId: string): Promise<void> {
  resumeByThread.delete(sessionId);
  await stopCodexSession(sessionId);
}

export function bindCodexSession(
  threadId: string,
  providerSessionId: string,
  cwd: string,
): void {
  const providerThreadId = providerSessionId.trim();
  if (!threadId || !providerThreadId || !cwd.trim()) return;
  resumeByThread.set(threadId, { threadId: providerThreadId, cwd });
}

async function ensureLive(input: HarnessSessionInput): Promise<Live> {
  const existing = liveByThread.get(input.sessionId);
  if (existing && existing.cwd === input.cwd) {
    existing.onEvent = input.onEvent;
    existing.runtimeMode = input.runtimeMode;
    return existing;
  }
  if (existing) {
    resumeByThread.delete(input.sessionId);
    await stopCodexSession(input.sessionId);
  }

  const resume = resumeByThread.get(input.sessionId);
  const canResume = resume != null && resume.cwd === input.cwd;
  if (resume && resume.cwd !== input.cwd) {
    resumeByThread.delete(input.sessionId);
  }

  const { path } = await resolveCodexBinaryImpl();
  const liveRef: { current: Live | null } = { current: null };

  const rpc = new JsonRpcClient(
    input.sessionId,
    {
      onNotification: (method, params) => {
        const live = liveRef.current;
        if (!live || live.muteUpdates) return;
        handleNotification(live, method, params);
      },
      onRequest: (id, method, params) => {
        const live = liveRef.current;
        if (!live) return;
        void handleServerRequest(live, id, method, params);
      },
    },
    { includeJsonrpc: false, label: "codex" },
  );

  watchChild(
    input.sessionId,
    (line) => rpc.pushLine(line),
    (code) => {
      rpc.close(new Error("Codex app-server exited"));
      liveByThread.delete(input.sessionId);
      input.onEvent({ type: "session.ended", code });
      const live = liveRef.current;
      live?.turnFailed?.(new Error("Codex app-server exited"));
      if (live) {
        live.turnDone = null;
        live.turnFailed = null;
      }
    },
  );

  await spawnChild(input.sessionId, path, ["app-server"], input.cwd);

  try {
    await rpc.request("initialize", {
      clientInfo: {
        name: "monocode",
        title: "MonoCode",
        version: "0.1.0",
      },
      capabilities: {
        experimentalApi: true,
      },
    });
    await rpc.notify("initialized", undefined);

    const model = nativeModelId(input.model);
    const serviceTier = input.modelSettings?.serviceTier;
    const effort = input.modelSettings?.reasoningEffort;

    let threadId: string | undefined;
    let didResume = false;

    if (canResume && resume) {
      try {
        const opened = await rpc.request<{ thread?: { id?: string } }>(
          "thread/resume",
          {
            threadId: resume.threadId,
            ...buildThreadStartParams({
              cwd: input.cwd,
              runtimeMode: input.runtimeMode,
              model,
              serviceTier,
            }),
          },
        );
        threadId = opened.thread?.id ?? resume.threadId;
        didResume = true;
      } catch (error) {
        if (!isRecoverableThreadResumeError(error)) throw error;
        threadId = undefined;
      }
    }

    if (!threadId) {
      const opened = await rpc.request<{ thread?: { id?: string } }>(
        "thread/start",
        buildThreadStartParams({
          cwd: input.cwd,
          runtimeMode: input.runtimeMode,
          model,
          serviceTier,
        }),
      );
      threadId = opened.thread?.id?.trim();
    }

    if (!threadId) throw new Error("Codex did not return a thread id");

    // Suppress unused warning for effort until first turn applies it.
    void effort;

    const live: Live = {
      rpc,
      threadId,
      cwd: input.cwd,
      runtimeMode: input.runtimeMode,
      planning: input.intent === "plan",
      onEvent: input.onEvent,
      approvals: new Map(),
      nextApprovalUiId: 1,
      cancelled: false,
      muteUpdates: didResume,
      activeTurnId: null,
      turns: Promise.resolve(),
      turnDone: null,
      turnFailed: null,
      turnEndPending: false,
      emittedAssistant: "",
      emittedReasoning: "",
    };
    liveRef.current = live;
    liveByThread.set(input.sessionId, live);
    resumeByThread.set(input.sessionId, {
      threadId,
      cwd: input.cwd,
    });
    live.onEvent({
      type: "session.providerBound",
      providerSessionId: threadId,
    });
    live.onEvent({ type: "session.started" });
    return live;
  } catch (error) {
    rpc.close(error instanceof Error ? error : new Error(String(error)));
    await stopCodexSession(input.sessionId);
    throw error;
  }
}

async function runTurn(live: Live, input: SendTurnInput): Promise<void> {
  const model = nativeModelId(input.model);
  const effort = input.modelSettings?.reasoningEffort;
  const serviceTier = input.modelSettings?.serviceTier;
  const attachments = await codexAttachments(input.attachments ?? []);

  const params = buildTurnStartParams({
    threadId: live.threadId,
    runtimeMode: input.runtimeMode,
    prompt: input.text.trim() || undefined,
    attachments,
    model,
    effort,
    serviceTier,
    intent: input.intent,
  });

  if (
    (!params.input ||
      (Array.isArray(params.input) && params.input.length === 0)) &&
    !input.text.trim() &&
    attachments.length === 0
  ) {
    return;
  }

  live.emittedAssistant = "";
  live.emittedReasoning = "";

  const turnPromise = new Promise<void>((resolve, reject) => {
    live.turnDone = resolve;
    live.turnFailed = reject;
  });
  settlePendingTurn(live);

  try {
    const response = await live.rpc.request<{ turn?: { id?: string } }>(
      "turn/start",
      params,
    );
    const turnId = response.turn?.id;
    if (turnId) {
      live.activeTurnId = live.activeTurnId ?? turnId;
    }
    settlePendingTurn(live);
    await turnPromise;
  } catch (error) {
    if (live.cancelled) return;
    live.onEvent({
      type: "session.error",
      message: error instanceof Error ? error.message : String(error),
    });
    throw error;
  } finally {
    live.turnDone = null;
    live.turnFailed = null;
  }
}

async function runCompaction(live: Live): Promise<void> {
  live.emittedAssistant = "";
  live.emittedReasoning = "";
  const turnPromise = new Promise<void>((resolve, reject) => {
    live.turnDone = resolve;
    live.turnFailed = reject;
  });
  settlePendingTurn(live);

  try {
    await live.rpc.request("thread/compact/start", {
      threadId: live.threadId,
    });
    settlePendingTurn(live);
    await turnPromise;
  } finally {
    live.turnDone = null;
    live.turnFailed = null;
  }
}

function handleNotification(live: Live, method: string, params: unknown): void {
  // A Codex turn is a sequence of items. Completing an agentMessage does not
  // mean the turn is over — more tools and messages can still arrive. Only
  // turn/completed (and turn/aborted) settle sendCodexTurn, which is what the
  // UI uses for busy / stop / "Working for".
  const mapped = mapCodexNotification(method, params);
  const snapshot = method === "item/completed";
  for (const event of mapped.events) {
    if (event.type === "message.delta") {
      publishCodexText(live, "assistant", event.text, snapshot);
      continue;
    }
    if (event.type === "reasoning.delta") {
      publishCodexText(live, "reasoning", event.text, snapshot);
      continue;
    }
    live.onEvent(event);
  }
  if (mapped.activeTurnId !== undefined) {
    live.activeTurnId = mapped.activeTurnId;
  }
  if (mapped.turnCompleted) {
    finishActiveTurn(live);
  }
}

function publishCodexText(
  live: Live,
  role: "assistant" | "reasoning",
  text: string,
  snapshot: boolean,
): void {
  const already =
    role === "assistant" ? live.emittedAssistant : live.emittedReasoning;
  const emit = snapshot ? snapshotRemainder(already, text) : text;
  if (!emit) return;
  if (role === "assistant") {
    live.emittedAssistant = joinStreamText(already, emit);
    live.onEvent({ type: "message.delta", text: emit });
    return;
  }
  live.emittedReasoning = joinStreamText(already, emit);
  live.onEvent({ type: "reasoning.delta", text: emit });
}

function finishActiveTurn(live: Live, extraEvents: HarnessEvent[] = []): void {
  live.turnEndPending = false;
  live.activeTurnId = null;
  live.emittedAssistant = "";
  live.emittedReasoning = "";
  for (const event of extraEvents) {
    live.onEvent(event);
  }
  const done = live.turnDone;
  const failed = live.turnFailed;
  live.turnDone = null;
  live.turnFailed = null;
  if (done) {
    done();
    return;
  }
  if (!failed) {
    live.turnEndPending = true;
  }
}

function settlePendingTurn(live: Live): void {
  if (!live.turnEndPending || !live.turnDone) return;
  finishActiveTurn(live);
}

async function handleServerRequest(
  live: Live,
  id: JsonRpcId,
  method: string,
  params: unknown,
): Promise<void> {
  if (method === "item/tool/requestUserInput") {
    await live.rpc.respond(id, { answers: {} }).catch(() => undefined);
    return;
  }

  const uiId = live.nextApprovalUiId++;
  const mapped = mapApprovalRequest(method, params, uiId);
  if (!mapped) {
    // Unknown server request — decline/cancel safely when possible.
    if (method.includes("requestApproval") || method.includes("Approval")) {
      await live.rpc
        .respond(id, { decision: "decline" })
        .catch(() => undefined);
      return;
    }
    if (method === "item/permissions/requestApproval") {
      await live.rpc.respond(id, { permissions: {} }).catch(() => undefined);
      return;
    }
    await live.rpc.respond(id, {}).catch(() => undefined);
    return;
  }

  if (live.planning) {
    // Plan turns run in a non-escalating read-only sandbox. If an older
    // app-server still asks for broader access, deny it silently instead of
    // leaking a Supervised approval prompt into the user's selected mode.
    if (method === "item/permissions/requestApproval") {
      await live.rpc.respond(id, { permissions: {} }).catch(() => undefined);
    } else {
      await live.rpc
        .respond(id, {
          decision: toCodexApprovalDecision("deny", mapped.kind),
        })
        .catch(() => undefined);
    }
    return;
  }

  if (method === "item/permissions/requestApproval") {
    // Auto-deny extra permission grants in supervised; allow in full-access.
    if (live.runtimeMode === "full-access") {
      const rec = asRecord(params);
      const permissions = rec?.permissions ?? {};
      await live.rpc.respond(id, {
        scope: "session",
        permissions,
      });
      return;
    }
    if (live.runtimeMode === "supervised") {
      live.onEvent(mapped.event);
      const decision = await waitApproval(live, uiId, id, mapped.kind);
      live.onEvent({
        type: "approval.resolved",
        requestId: uiId,
        decision,
      });
      if (decision === "allow") {
        const rec = asRecord(params);
        await live.rpc.respond(id, {
          scope: "turn",
          permissions: rec?.permissions ?? {},
        });
      } else {
        await live.rpc.respond(id, { permissions: {} });
      }
      return;
    }
    // auto / auto-accept: grant requested permissions for the turn.
    const rec = asRecord(params);
    await live.rpc.respond(id, {
      scope: "turn",
      permissions: rec?.permissions ?? {},
    });
    return;
  }

  const auto = autoApproval(live.runtimeMode, mapped.kind);
  if (auto) {
    await live.rpc.respond(id, {
      decision: toCodexApprovalDecision(auto, mapped.kind),
    });
    return;
  }

  live.onEvent(mapped.event);
  const decision = await waitApproval(live, uiId, id, mapped.kind);
  live.onEvent({
    type: "approval.resolved",
    requestId: uiId,
    decision,
  });
  await live.rpc.respond(id, {
    decision: toCodexApprovalDecision(decision, mapped.kind),
  });
}

function waitApproval(
  live: Live,
  uiId: number,
  rpcId: JsonRpcId,
  kind: CodexApprovalKind,
): Promise<ApprovalDecision> {
  return new Promise<ApprovalDecision>((resolve) => {
    live.approvals.set(uiId, { rpcId, kind, resolve });
  }).finally(() => {
    live.approvals.delete(uiId);
  });
}

function autoApproval(
  runtimeMode: RuntimeMode,
  kind: CodexApprovalKind,
): ApprovalDecision | null {
  if (runtimeMode === "supervised") return null;
  if (runtimeMode === "full-access") return "allow";
  if (runtimeMode === "auto") {
    // auto_review is set on the server; still prompt if Codex asks.
    return null;
  }
  // auto-accept-edits: auto file changes, ask for commands.
  if (kind === "file-change") return "allow";
  return null;
}

async function codexAttachments(
  files: Attachment[],
): Promise<Array<{ type: "image"; url: string }>> {
  const out: Array<{ type: "image"; url: string }> = [];
  for (const file of files) {
    if (!file.data) continue;
    if (!file.mimeType.startsWith("image/")) continue;
    out.push({
      type: "image",
      url: `data:${file.mimeType};base64,${file.data}`,
    });
  }
  return out;
}

/** Exported for tests. */
export function __codexTestReset(): void {
  liveByThread.clear();
  resumeByThread.clear();
  cancelledThreads.clear();
}

export function __codexTestResumeMap(): Map<string, Resume> {
  return resumeByThread;
}
