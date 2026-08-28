import { nativeModelId } from "../models";
import {
  killChild,
  spawnChild,
  unwatchChild,
  watchChild,
  writeChild,
} from "./child";
import type { PiFlavor } from "./piFlavor";
import { PiRpc } from "./piClient";
import {
  agentEndWillRetry,
  asRecord,
  assistantDeltaFromEvent,
  buildPiPrompt,
  buildPiSpawnArgs,
  buildPiSteer,
  contextFromSessionStats,
  contextFromUsage,
  extensionUiResponse,
  extensionUiTitle,
  isAgentSettled,
  isPiThinkingLevel,
  needsExtensionUiReply,
  parseExtensionUiRequest,
  parsePiModelRef,
  piNativeId,
  previewFromTool,
  providerSessionIdFromState,
  sessionFromState,
  statusFromPiEvent,
  stringField,
  summarizeToolRequest,
  toolCallDeltaFromEvent,
  toolCallEndFromEvent,
  toolCallStartFromEvent,
  toolExecutionEndFromEvent,
  toolExecutionStartFromEvent,
  toolExecutionUpdateFromEvent,
  toolKindFromName,
  toolTitle,
  tryParseJsonRecord,
  turnErrorFromEvent,
  type PiExtensionUiRequest,
} from "./piProtocol";
import type { ApprovalDecision, HarnessEvent, SendTurnInput, SteerTurnInput } from "./types";

type PendingApproval = {
  request: PiExtensionUiRequest;
  resolve: (decision: ApprovalDecision) => void;
};

type InFlightTool = {
  id: string;
  name: string;
  input: Record<string, unknown>;
  partialJson: string;
  title: string;
};

type Live = {
  rpc: PiRpc;
  cwd: string;
  providerSessionId: string;
  contextWindow?: number;
  nativeModel: string;
  thinking: string;
  onEvent: (event: HarnessEvent) => void;
  approvals: Map<number, PendingApproval>;
  nextApprovalUiId: number;
  toolsByIndex: Map<number, InFlightTool>;
  toolsById: Map<string, InFlightTool>;
  cancelled: boolean;
  muteUpdates: boolean;
  compacting: boolean;
  retrying: boolean;
  settling: boolean;
  settleToken: number;
  turns: Promise<void>;
  turnDone: (() => void) | null;
  turnFailed: ((error: Error) => void) | null;
  turnEndPending: boolean;
  activeTurn: boolean;
  emittedAssistant: string;
  emittedReasoning: string;
  /** Reason the turn failed, held until we know it is not being retried. */
  turnError: string | null;
};

type Resume = {
  sessionId: string;
  cwd: string;
};

const INIT_TIMEOUT_MS = 45_000;
const STATS_TIMEOUT_MS = 4_000;

type FlavorState = {
  liveByThread: Map<string, Live>;
  resumeByThread: Map<string, Resume>;
  cancelledThreads: Set<string>;
  resolveBinary: () => Promise<{ path: string }>;
};

/**
 * Pi and omp each get their own child processes and resume tables, so a live
 * session on one never collides with the other.
 */
const stateByFlavor = new Map<string, FlavorState>();

function stateFor(flavor: PiFlavor): FlavorState {
  let state = stateByFlavor.get(flavor.id);
  if (!state) {
    state = {
      liveByThread: new Map(),
      resumeByThread: new Map(),
      cancelledThreads: new Set(),
      resolveBinary: flavor.resolveBinary,
    };
    stateByFlavor.set(flavor.id, state);
  }
  return state;
}

/** Test seam. */
export function setPiBinaryResolver(
  flavor: PiFlavor,
  fn: () => Promise<{ path: string }>,
): void {
  stateFor(flavor).resolveBinary = fn;
}

/**
 * Live Pi adapter. Spawns `pi --mode rpc` with the user's config and extensions
 * loaded (no `--no-extensions`). Todos/subagents packages in `~/.pi/agent`
 * keep working; TUI-only widgets do not appear in MonoCode.
 */
export async function sendTurn(
  flavor: PiFlavor,
  input: SendTurnInput,
): Promise<void> {
  const { cancelledThreads } = stateFor(flavor);
  let live: Live;
  try {
    live = await ensureLive(flavor, input);
  } catch (error) {
    cancelledThreads.delete(input.sessionId);
    throw error;
  }
  if (cancelledThreads.delete(input.sessionId)) return;

  live.onEvent = input.onEvent;
  live.turns = live.turns.catch(() => undefined).then(async () => {
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

export async function steerTurn(
  flavor: PiFlavor,
  input: SteerTurnInput,
): Promise<void> {
  const live = stateFor(flavor).liveByThread.get(input.sessionId);
  if (!live?.activeTurn) throw new Error("No active turn to steer");
  const message = input.text.trim();
  const command = buildPiSteer({
    text: message,
    attachments: input.attachments,
  });
  if (!message && !Array.isArray(command.images)) return;
  await live.rpc.request(command);
}

export function respondApproval(
  flavor: PiFlavor,
  sessionId: string,
  requestId: number,
  decision: ApprovalDecision,
): void {
  const live = stateFor(flavor).liveByThread.get(sessionId);
  const pending = live?.approvals.get(requestId);
  if (!pending) return;
  pending.resolve(decision);
}

export async function cancelTurn(
  flavor: PiFlavor,
  sessionId: string,
): Promise<void> {
  const { liveByThread, cancelledThreads } = stateFor(flavor);
  const live = liveByThread.get(sessionId);
  if (!live) {
    cancelledThreads.add(sessionId);
    return;
  }
  live.cancelled = true;
  live.muteUpdates = true;
  live.settleToken += 1;
  for (const [, pending] of live.approvals) pending.resolve("deny");
  live.approvals.clear();
  await live.rpc.request({ type: "abort" }, 5_000).catch(() => undefined);
  finishActiveTurn(live, [
    { type: "message.completed" },
    { type: "reasoning.completed" },
  ]);
}

export async function stopSession(
  flavor: PiFlavor,
  sessionId: string,
): Promise<void> {
  const { liveByThread, cancelledThreads } = stateFor(flavor);
  cancelledThreads.delete(sessionId);
  const live = liveByThread.get(sessionId);
  liveByThread.delete(sessionId);
  if (live) {
    live.muteUpdates = true;
    live.settleToken += 1;
    for (const [, pending] of live.approvals) pending.resolve("deny");
    live.approvals.clear();
    live.activeTurn = false;
    live.turnDone?.();
    live.turnDone = null;
    live.turnFailed = null;
    live.rpc.close();
  }
  unwatchChild(sessionId);
  await killChild(sessionId).catch(() => undefined);
}

export async function forgetSession(
  flavor: PiFlavor,
  sessionId: string,
): Promise<void> {
  stateFor(flavor).resumeByThread.delete(sessionId);
  await stopSession(flavor, sessionId);
}

export function bindSession(
  flavor: PiFlavor,
  threadId: string,
  providerSessionId: string,
  cwd: string,
): void {
  const sessionId = providerSessionId.trim();
  if (!threadId || !sessionId || !cwd.trim()) return;
  stateFor(flavor).resumeByThread.set(threadId, { sessionId, cwd });
}

async function ensureLive(
  flavor: PiFlavor,
  input: SendTurnInput,
): Promise<Live> {
  const { liveByThread, resumeByThread } = stateFor(flavor);
  const existing = liveByThread.get(input.sessionId);
  if (existing && existing.cwd === input.cwd) {
    existing.onEvent = input.onEvent;
    await applyModel(existing, input);
    return existing;
  }
  if (existing) {
    resumeByThread.delete(input.sessionId);
    await stopSession(flavor, input.sessionId);
  }

  const resume = resumeByThread.get(input.sessionId);
  const canResume = resume != null && resume.cwd === input.cwd;
  if (resume && resume.cwd !== input.cwd) {
    resumeByThread.delete(input.sessionId);
  }

  try {
    return await startLive(
      flavor,
      input,
      canResume ? resume?.sessionId : undefined,
    );
  } catch (error) {
    if (!canResume) throw error;
    resumeByThread.delete(input.sessionId);
    await stopSession(flavor, input.sessionId);
    return startLive(flavor, input, undefined);
  }
}

async function startLive(
  flavor: PiFlavor,
  input: SendTurnInput,
  resume: string | undefined,
): Promise<Live> {
  const state = stateFor(flavor);
  const { liveByThread } = state;
  const { path } = await state.resolveBinary();
  const native = nativeModelId(input.model);
  const modelRef = parsePiModelRef(native);
  const liveRef: { current: Live | null } = { current: null };

  const rpc = new PiRpc(input.sessionId, (rec) => {
    const current = liveRef.current;
    if (!current) return;
    handleFrame(flavor, input.sessionId, current, rec);
  }, flavor.label);

  const live: Live = {
    rpc,
    cwd: input.cwd,
    providerSessionId: resume ?? "",
    nativeModel: native,
    thinking: input.modelSettings?.thinking ?? "",
    onEvent: input.onEvent,
    approvals: new Map(),
    nextApprovalUiId: 1,
    toolsByIndex: new Map(),
    toolsById: new Map(),
    cancelled: false,
    muteUpdates: false,
    compacting: false,
    retrying: false,
    settling: false,
    settleToken: 0,
    turns: Promise.resolve(),
    turnDone: null,
    turnFailed: null,
    turnEndPending: false,
    activeTurn: false,
    emittedAssistant: "",
    emittedReasoning: "",
    turnError: null,
  };
  liveRef.current = live;

  watchChild(
    input.sessionId,
    (line) => rpc.pushLine(line),
    (code) => {
      rpc.close(new Error(`${flavor.label} exited`));
      liveByThread.delete(input.sessionId);
      input.onEvent({ type: "session.ended", code });
      const current = liveRef.current;
      current?.turnFailed?.(new Error(`${flavor.label} exited`));
      if (current) {
        current.turnDone = null;
        current.turnFailed = null;
      }
    },
    (line) => {
      console.debug(`[${flavor.id}]`, line);
    },
  );

  await spawnChild(
    input.sessionId,
    path,
    buildPiSpawnArgs(flavor, {
      resume,
      model: modelRef ? native : undefined,
    }),
    input.cwd,
  );

  liveByThread.set(input.sessionId, live);

  try {
    const stateFrame = await rpc.request(
      { type: "get_state" },
      INIT_TIMEOUT_MS,
    );
    bindState(flavor, input.sessionId, live, stateFrame.data);
    await applyModel(live, input);
    if (live.providerSessionId) {
      live.onEvent({
        type: "session.providerBound",
        providerSessionId: live.providerSessionId,
      });
    }
    live.onEvent({ type: "session.started" });
    return live;
  } catch (error) {
    await stopSession(flavor, input.sessionId);
    throw error;
  }
}

async function runTurn(live: Live, input: SendTurnInput): Promise<void> {
  await applyModel(live, input);
  live.emittedAssistant = "";
  live.emittedReasoning = "";
  live.turnError = null;
  live.toolsByIndex.clear();
  live.toolsById.clear();
  live.compacting = false;
  live.retrying = false;

  const turnPromise = new Promise<void>((resolve, reject) => {
    live.turnDone = resolve;
    live.turnFailed = reject;
  });
  live.activeTurn = true;
  settlePendingTurn(live);

  try {
    await live.rpc.request(
      buildPiPrompt({
        text: input.text,
        attachments: input.attachments,
      }),
    );
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

function handleFrame(
  flavor: PiFlavor,
  sessionId: string,
  live: Live,
  rec: Record<string, unknown>,
): void {
  const ui = parseExtensionUiRequest(rec);
  if (ui) {
    void handleExtensionUi(sessionId, live, ui);
    return;
  }
  if (live.muteUpdates) return;

  const type = stringField(rec, "type");
  if (type === "compaction_start") live.compacting = true;
  if (type === "compaction_end") live.compacting = false;
  if (type === "auto_retry_start") live.retrying = true;
  if (type === "auto_retry_end") live.retrying = false;

  const status = statusFromPiEvent(rec);
  if (status) live.onEvent({ type: "status", text: status });

  const turnError = turnErrorFromEvent(rec);
  if (turnError !== null) live.turnError = turnError;

  const context = contextFromUsage(rec, live.contextWindow);
  if (context) live.onEvent({ type: "context", ...context });

  const delta = assistantDeltaFromEvent(rec);
  if (delta) {
    if (delta.kind === "text") {
      live.emittedAssistant += delta.text;
      live.onEvent({ type: "message.delta", text: delta.text });
    } else {
      live.emittedReasoning += delta.text;
      live.onEvent({ type: "reasoning.delta", text: delta.text });
    }
  }

  const started = toolCallStartFromEvent(rec);
  if (started) {
    upsertTool(live, started.id, started.name, {}, started.index);
  }

  const jsonDelta = toolCallDeltaFromEvent(rec);
  if (jsonDelta) {
    const tool = live.toolsByIndex.get(jsonDelta.index);
    if (tool) {
      tool.partialJson += jsonDelta.delta;
      const parsed = tryParseJsonRecord(tool.partialJson);
      if (parsed) updateTool(live, tool, parsed);
    }
  }

  const ended = toolCallEndFromEvent(rec);
  if (ended) {
    upsertTool(live, ended.id, ended.name, ended.input);
  }

  const execStart = toolExecutionStartFromEvent(rec);
  if (execStart) {
    upsertTool(live, execStart.id, execStart.name, execStart.input);
    const tool = live.toolsById.get(execStart.id);
    if (tool) {
      live.onEvent({
        type: "tool.updated",
        callId: tool.id,
        title: tool.title,
        kind: toolKindFromName(tool.name),
        status: "running",
        preview: previewFromTool(tool.name, tool.input),
      });
    }
  }

  const execUpdate = toolExecutionUpdateFromEvent(rec);
  if (execUpdate) {
    const tool = live.toolsById.get(execUpdate.id);
    if (tool) {
      live.onEvent({
        type: "tool.updated",
        callId: tool.id,
        title: tool.title,
        kind: toolKindFromName(tool.name),
        status: "running",
        detail: execUpdate.detail,
        preview: previewFromTool(tool.name, tool.input, execUpdate.detail),
      });
    }
  }

  const execEnd = toolExecutionEndFromEvent(rec);
  if (execEnd) {
    const tool = live.toolsById.get(execEnd.id);
    if (tool) {
      live.onEvent({
        type: "tool.updated",
        callId: tool.id,
        title: tool.title,
        kind: toolKindFromName(tool.name),
        status: execEnd.isError ? "failed" : "completed",
        detail: execEnd.detail,
        preview: previewFromTool(tool.name, tool.input, execEnd.detail),
      });
    }
  }

  if (isAgentSettled(rec)) {
    flushTurnError(flavor, live);
    void settleTurn(live);
    return;
  }
  const willRetry = agentEndWillRetry(rec);
  // The retry carries the real answer, so the attempt it replaces stays quiet.
  if (willRetry === true) live.turnError = null;
  if (willRetry === false && !live.compacting && !live.retrying) {
    flushTurnError(flavor, live);
    void settleTurn(live);
  }
}

function flushTurnError(flavor: PiFlavor, live: Live): void {
  const message = live.turnError;
  if (message === null) return;
  live.turnError = null;
  live.onEvent({
    type: "session.error",
    message: message || `${flavor.label} turn failed`,
  });
}

async function settleTurn(live: Live): Promise<void> {
  if (live.settling || live.cancelled || live.muteUpdates) return;
  if (!live.activeTurn && !live.turnDone) return;
  live.settling = true;
  const token = live.settleToken;
  try {
    const stats = await live.rpc.request(
      { type: "get_session_stats" },
      STATS_TIMEOUT_MS,
    );
    if (live.settleToken === token && !live.cancelled) {
      const context = contextFromSessionStats(stats.data);
      if (context) live.onEvent({ type: "context", ...context });
    }
  } catch {
    // meter stays on the last streamed usage
  }
  if (live.settleToken === token && !live.cancelled) {
    finishActiveTurn(live, [
      { type: "message.completed" },
      { type: "reasoning.completed" },
    ]);
  }
  live.settling = false;
}

async function handleExtensionUi(
  sessionId: string,
  live: Live,
  request: PiExtensionUiRequest,
): Promise<void> {
  if (!needsExtensionUiReply(request)) {
    if (request.title) live.onEvent({ type: "status", text: request.title });
    return;
  }

  if (live.cancelled || live.muteUpdates) {
    await writeChild(
      sessionId,
      JSON.stringify(extensionUiResponse(request, "deny")),
    ).catch(() => undefined);
    return;
  }

  const uiId = live.nextApprovalUiId++;
  live.onEvent({
    type: "approval.requested",
    requestId: uiId,
    title: extensionUiTitle(request),
    kind: "other",
  });
  const decision = await new Promise<ApprovalDecision>((resolve) => {
    live.approvals.set(uiId, { request, resolve });
  });
  live.approvals.delete(uiId);
  live.onEvent({ type: "approval.resolved", requestId: uiId, decision });
  await writeChild(
    sessionId,
    JSON.stringify(extensionUiResponse(request, decision)),
  ).catch(() => undefined);
}

async function applyModel(live: Live, input: SendTurnInput): Promise<void> {
  const native = nativeModelId(input.model);
  const ref = parsePiModelRef(native);
  if (ref && native !== live.nativeModel) {
    const result = await live.rpc.request({
      type: "set_model",
      provider: ref.provider,
      modelId: ref.modelId,
    });
    live.nativeModel = native;
    const model = asRecord(result.data);
    const window = model && typeof model.contextWindow === "number"
      ? model.contextWindow
      : undefined;
    if (window && window > 0) live.contextWindow = window;
  } else if (ref) {
    live.nativeModel = native;
  }

  const thinking = input.modelSettings?.thinking;
  if (isPiThinkingLevel(thinking) && thinking !== live.thinking) {
    await live.rpc
      .request({ type: "set_thinking_level", level: thinking })
      .catch(() => undefined);
    live.thinking = thinking;
  }
}

function bindState(
  flavor: PiFlavor,
  sessionId: string,
  live: Live,
  data: unknown,
): void {
  const state = sessionFromState(data);
  const providerSessionId = providerSessionIdFromState(data);
  if (state.contextWindow) live.contextWindow = state.contextWindow;
  if (providerSessionId) {
    live.providerSessionId = providerSessionId;
    stateFor(flavor).resumeByThread.set(sessionId, {
      sessionId: providerSessionId,
      cwd: live.cwd,
    });
  }
  const model = asRecord(asRecord(data)?.model);
  const provider = stringField(model, "provider");
  const modelId = stringField(model, "id");
  if (provider && modelId && !live.nativeModel) {
    live.nativeModel = piNativeId(provider, modelId);
  }
}

function upsertTool(
  live: Live,
  id: string,
  name: string,
  input: Record<string, unknown>,
  index?: number,
): void {
  let tool = live.toolsById.get(id);
  if (!tool) {
    tool = {
      id,
      name,
      input,
      partialJson: "",
      title: toolTitle(name, input),
    };
    live.toolsById.set(id, tool);
    live.onEvent({
      type: "tool.started",
      callId: id,
      title: tool.title,
      kind: toolKindFromName(name),
      status: "pending",
      preview: previewFromTool(name, input),
    });
  } else if (Object.keys(input).length > 0) {
    updateTool(live, tool, input);
  }
  if (index != null && index >= 0) live.toolsByIndex.set(index, tool);
}

function updateTool(
  live: Live,
  tool: InFlightTool,
  input: Record<string, unknown>,
): void {
  tool.input = input;
  tool.title = toolTitle(tool.name, input);
  live.onEvent({
    type: "tool.updated",
    callId: tool.id,
    title: tool.title,
    kind: toolKindFromName(tool.name),
    status: "pending",
    detail: summarizeToolRequest(tool.name, input),
    preview: previewFromTool(tool.name, input),
  });
}

function finishActiveTurn(live: Live, extraEvents: HarnessEvent[] = []): void {
  if (!live.activeTurn && !live.turnDone) return;
  live.turnEndPending = false;
  live.activeTurn = false;
  for (const event of extraEvents) live.onEvent(event);
  const done = live.turnDone;
  const failed = live.turnFailed;
  live.turnDone = null;
  live.turnFailed = null;
  if (done) {
    done();
    return;
  }
  if (!failed) live.turnEndPending = true;
}

function settlePendingTurn(live: Live): void {
  if (!live.turnEndPending || !live.turnDone) return;
  finishActiveTurn(live);
}
