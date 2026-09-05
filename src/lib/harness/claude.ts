import { nativeModelId } from "../models";
import type { RuntimeMode } from "../session";
import { loadClaudeHooks } from "../settings";
import {
  killChild,
  resolveClaudeBinary,
  spawnChild,
  unwatchChild,
  watchChild,
  writeChild,
} from "./child";
import {
  askUserQuestionAllowInput,
  assistantModel,
  assistantTextBlocks,
  assistantToolUses,
  contextFromResult,
  contextUsedFromAssistant,
  buildClaudeSpawnArgs,
  buildClaudeUserMessage,
  buildControlRequest,
  buildControlResponse,
  claudeSettingsKey,
  extractAskUserQuestionTitle,
  extractExitPlanModePlan,
  inputJsonDeltaFromEvent,
  isAgentTaskType,
  isAsyncAgentLaunch,
  isClaudeUltracodeEffort,
  isSubagentMessage,
  isTerminalAgentTaskStatus,
  isTodoTool,
  normalizeClaudeCliEffort,
  parseBackgroundAgentTasks,
  parseControlCancelId,
  parseControlRequest,
  parseJsonLine,
  parseTaskNotification,
  parseTaskProgress,
  parseTaskStarted,
  parseTaskUpdated,
  parseToolProgress,
  taskListFromTodos,
  previewFromTool,
  resolveClaudeApiModelId,
  runtimeModeToPermission,
  sessionIdFromMessage,
  statusTextFromSystem,
  streamDeltaFromEvent,
  stringField,
  summarizeToolRequest,
  toClaudePermissionResult,
  toolKindFromName,
  toolResultsFromUserMessage,
  toolStartFromEvent,
  toolTitle,
  tryParseJsonRecord,
  turnStatusFromResult,
  type ClaudeCliSettings,
  type ClaudeControlRequest,
} from "./claudeProtocol";
import { isAgentToolName, subagentMetaFromInput } from "./preview";
import { joinStreamText, snapshotRemainder } from "./streamText";
import {
  questionPromptTitle,
  questionsFromUnknown,
  type UserQuestionReply,
} from "../userQuestion";
import type {
  ApprovalDecision,
  CompactContextInput,
  HarnessEvent,
  HarnessSessionInput,
  SendTurnInput,
  SteerTurnInput,
} from "./types";

/**
 * A PermissionRequest hook can decide before the user touches the prompt; Claude
 * then cancels the control request out from under us. That is not a rejection,
 * so it gets its own outcome instead of being folded into "deny".
 */
type ApprovalOutcome = ApprovalDecision | "cancelled";

type PendingApproval = {
  requestId: string;
  input: Record<string, unknown>;
  resolve: (decision: ApprovalOutcome) => void;
};

type PendingQuestion = {
  requestId: string;
  resolve: (reply: UserQuestionReply | "cancelled") => void;
};

type InFlightTool = {
  id: string;
  name: string;
  input: Record<string, unknown>;
  partialJson: string;
  title: string;
};

type LiveAgentTask = {
  taskId: string;
  toolUseId?: string;
  description: string;
  backgrounded: boolean;
};

/**
 * One stream of content blocks: the parent's, or a subagent's. Each keeps its
 * own tool index map and emitted-text tally, since a subagent's block indexes
 * and snapshots start over from zero and would collide with the parent's.
 */
type StreamScope = {
  toolsByIndex: Map<number, InFlightTool>;
  toolsById: Map<string, InFlightTool>;
  emittedAssistant: string;
  emittedReasoning: string;
};

/** Where a line's events go: straight out, or nested under an Agent call. */
type StreamTarget = {
  scope: StreamScope;
  emit: (event: HarnessEvent) => void;
  /** The Agent tool call this stream belongs to; absent for the parent. */
  parentId?: string;
};

type SubagentMetaPatch = {
  agentType?: string;
  prompt?: string;
  model?: string;
  background?: boolean;
};

const SUBAGENT_META_KEYS = [
  "agentType",
  "prompt",
  "model",
  "background",
] as const satisfies readonly (keyof SubagentMetaPatch)[];

type Live = {
  cwd: string;
  claudeSessionId: string;
  runtimeMode: RuntimeMode;
  planning: boolean;
  settingsKey: string;
  onEvent: (event: HarnessEvent) => void;
  approvals: Map<number, PendingApproval>;
  questions: Map<number, PendingQuestion>;
  nextApprovalUiId: number;
  nextControlId: number;
  toolsByIndex: Map<number, InFlightTool>;
  toolsById: Map<string, InFlightTool>;
  /** Subagent streams by the Agent tool call that spawned them. */
  subagents: Map<string, StreamScope>;
  /** What each subagent has already been told about itself; repeats are dropped. */
  subagentMeta: Map<string, SubagentMetaPatch>;
  /** Agent calls that reached a terminal status; late lines for them are dropped. */
  finishedAgents: Set<string>;
  agentTasks: Map<string, LiveAgentTask>;
  /**
   * Agent calls whose tool result was only a launch receipt. The real answer
   * arrives through the task lifecycle, so the turn waits on these too.
   */
  asyncAgentTools: Set<string>;
  turnResultSeen: boolean;
  cancelled: boolean;
  muteUpdates: boolean;
  turns: Promise<void>;
  turnDone: (() => void) | null;
  turnFailed: ((error: Error) => void) | null;
  turnEndPending: boolean;
  activeTurn: boolean;
  initDone: (() => void) | null;
  initialized: boolean;
  emittedAssistant: string;
  emittedReasoning: string;
  manualCompaction: boolean;
  compactionConfirmed: boolean;
};

type Resume = {
  sessionId: string;
  cwd: string;
};

const INIT_TIMEOUT_MS = 8_000;

const liveByThread = new Map<string, Live>();
const resumeByThread = new Map<string, Resume>();
const cancelledThreads = new Set<string>();

let resolveClaudeBinaryImpl: () => Promise<{ path: string }> =
  resolveClaudeBinary;

/** Test seam. */
export function setClaudeBinaryResolver(
  fn: () => Promise<{ path: string }>,
): void {
  resolveClaudeBinaryImpl = fn;
}

export async function sendClaudeTurn(input: SendTurnInput): Promise<void> {
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

export async function compactClaudeContext(
  input: CompactContextInput,
): Promise<void> {
  const settingsKey = settingsKeyFor(input);
  let live = liveByThread.get(input.sessionId);
  if (!live || live.cwd !== input.cwd || live.settingsKey !== settingsKey) {
    live = await ensureLive(input);
  }
  if (cancelledThreads.delete(input.sessionId)) return;

  live.onEvent = input.onEvent;
  live.runtimeMode = input.runtimeMode;
  live.turns = live.turns
    .catch(() => undefined)
    .then(async () => {
      live.cancelled = false;
      live.muteUpdates = false;
      live.manualCompaction = true;
      live.compactionConfirmed = false;
      try {
        await runTurn(live, {
          ...input,
          modelSettings: undefined,
          text: "/compact",
          attachments: [],
        });
        if (!live.compactionConfirmed) {
          throw new Error("Claude Code did not confirm context compaction");
        }
      } catch (error) {
        if (live.cancelled) return;
        throw error;
      } finally {
        live.manualCompaction = false;
      }
    });
  await live.turns;
}

export async function steerClaudeTurn(input: SteerTurnInput): Promise<void> {
  const live = liveByThread.get(input.sessionId);
  if (!live?.activeTurn) throw new Error("No active turn to steer");

  const message = buildClaudeUserMessage({
    text: input.text,
    attachments: input.attachments,
    effort: input.modelSettings?.effort,
  });
  const content = (message.message as { content: unknown[] }).content;
  if (content.length === 0) return;

  await writeJson(input.sessionId, message);
}

export function respondClaudeApproval(
  sessionId: string,
  requestId: number,
  decision: ApprovalDecision,
): void {
  const live = liveByThread.get(sessionId);
  const pending = live?.approvals.get(requestId);
  if (!pending) return;
  pending.resolve(decision);
}

export function respondClaudeQuestion(
  sessionId: string,
  requestId: number,
  reply: UserQuestionReply,
): void {
  const live = liveByThread.get(sessionId);
  const pending = live?.questions.get(requestId);
  if (!pending) return;
  pending.resolve(reply);
}

export async function cancelClaudeTurn(sessionId: string): Promise<void> {
  const live = liveByThread.get(sessionId);
  if (!live) {
    cancelledThreads.add(sessionId);
    return;
  }
  live.cancelled = true;
  live.muteUpdates = true;
  for (const [, pending] of live.approvals) pending.resolve("deny");
  live.approvals.clear();
  for (const [, pending] of live.questions)
    pending.resolve({ kind: "skipped" });
  live.questions.clear();
  await writeJson(
    sessionId,
    buildControlRequest(nextControlId(live), { subtype: "interrupt" }),
  ).catch(() => undefined);
  finishActiveTurn(live, [
    { type: "message.completed" },
    { type: "reasoning.completed" },
  ]);
}

export async function stopClaudeSession(sessionId: string): Promise<void> {
  cancelledThreads.delete(sessionId);
  const live = liveByThread.get(sessionId);
  liveByThread.delete(sessionId);
  if (live) {
    live.muteUpdates = true;
    for (const [, pending] of live.approvals) pending.resolve("deny");
    live.approvals.clear();
    for (const [, pending] of live.questions)
      pending.resolve({ kind: "skipped" });
    live.questions.clear();
    live.activeTurn = false;
    live.turnDone?.();
    live.turnDone = null;
    live.turnFailed = null;
    live.initDone?.();
    live.initDone = null;
  }
  unwatchChild(sessionId);
  await killChild(sessionId).catch(() => undefined);
}

export async function forgetClaudeSession(sessionId: string): Promise<void> {
  resumeByThread.delete(sessionId);
  await stopClaudeSession(sessionId);
}

export function bindClaudeSession(
  threadId: string,
  providerSessionId: string,
  cwd: string,
): void {
  const sessionId = providerSessionId.trim();
  if (!threadId || !sessionId || !cwd.trim()) return;
  resumeByThread.set(threadId, { sessionId, cwd });
}

async function ensureLive(input: HarnessSessionInput): Promise<Live> {
  const settingsKey = settingsKeyFor(input);
  const planning = input.intent === "plan";
  const existing = liveByThread.get(input.sessionId);
  if (
    existing &&
    existing.cwd === input.cwd &&
    existing.settingsKey === settingsKey &&
    existing.planning === planning
  ) {
    existing.onEvent = input.onEvent;
    existing.runtimeMode = input.runtimeMode;
    return existing;
  }
  if (existing) {
    if (existing.cwd !== input.cwd || existing.settingsKey !== settingsKey) {
      resumeByThread.delete(input.sessionId);
    }
    await stopClaudeSession(input.sessionId);
  }

  const resume = resumeByThread.get(input.sessionId);
  const canResume = resume != null && resume.cwd === input.cwd;
  if (resume && resume.cwd !== input.cwd) {
    resumeByThread.delete(input.sessionId);
  }

  const { path } = await resolveClaudeBinaryImpl();
  const liveRef: { current: Live | null } = { current: null };
  const claudeSessionId =
    canResume && resume ? resume.sessionId : crypto.randomUUID();
  const launch = launchOptions(
    input,
    canResume ? resume?.sessionId : undefined,
    claudeSessionId,
  );

  const live: Live = {
    cwd: input.cwd,
    claudeSessionId,
    runtimeMode: input.runtimeMode,
    planning,
    settingsKey,
    onEvent: input.onEvent,
    approvals: new Map(),
    questions: new Map(),
    nextApprovalUiId: 1,
    nextControlId: 1,
    toolsByIndex: new Map(),
    toolsById: new Map(),
    subagents: new Map(),
    subagentMeta: new Map(),
    finishedAgents: new Set(),
    agentTasks: new Map(),
    asyncAgentTools: new Set(),
    turnResultSeen: false,
    cancelled: false,
    muteUpdates: false,
    turns: Promise.resolve(),
    turnDone: null,
    turnFailed: null,
    turnEndPending: false,
    activeTurn: false,
    initDone: null,
    initialized: false,
    emittedAssistant: "",
    emittedReasoning: "",
    manualCompaction: false,
    compactionConfirmed: false,
  };
  liveRef.current = live;

  watchChild(
    input.sessionId,
    (line) => {
      const current = liveRef.current;
      if (!current) return;
      handleLine(input.sessionId, current, line);
    },
    (code) => {
      liveByThread.delete(input.sessionId);
      input.onEvent({ type: "session.ended", code });
      const current = liveRef.current;
      current?.turnFailed?.(new Error("Claude Code exited"));
      current?.initDone?.();
      if (current) {
        current.turnDone = null;
        current.turnFailed = null;
        current.initDone = null;
      }
    },
  );

  await spawnChild(
    input.sessionId,
    path,
    buildClaudeSpawnArgs(launch),
    input.cwd,
  );

  liveByThread.set(input.sessionId, live);
  resumeByThread.set(input.sessionId, {
    sessionId: claudeSessionId,
    cwd: input.cwd,
  });

  try {
    await writeJson(
      input.sessionId,
      buildControlRequest(nextControlId(live), { subtype: "initialize" }),
    );
    await waitForInit(live, INIT_TIMEOUT_MS);
    live.onEvent({
      type: "session.providerBound",
      providerSessionId: live.claudeSessionId,
    });
    live.onEvent({ type: "session.started" });
    return live;
  } catch (error) {
    await stopClaudeSession(input.sessionId);
    throw error;
  }
}

async function runTurn(live: Live, input: SendTurnInput): Promise<void> {
  const effort = input.modelSettings?.effort;
  const message = buildClaudeUserMessage({
    text: input.text,
    attachments: input.attachments,
    effort,
  });
  const content = (message.message as { content: unknown[] }).content;
  if (content.length === 0) return;

  live.emittedAssistant = "";
  live.emittedReasoning = "";
  live.toolsByIndex.clear();
  live.toolsById.clear();
  live.subagents.clear();
  live.subagentMeta.clear();
  live.finishedAgents.clear();
  live.agentTasks.clear();
  live.asyncAgentTools.clear();
  live.turnResultSeen = false;

  const turnPromise = new Promise<void>((resolve, reject) => {
    live.turnDone = resolve;
    live.turnFailed = reject;
  });
  live.activeTurn = true;
  settlePendingTurn(live);

  try {
    await writeJson(input.sessionId, message);
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

function handleLine(sessionId: string, live: Live, line: string): void {
  const rec = parseJsonLine(line);
  if (!rec) return;

  const type = stringField(rec, "type");
  if (type === "keep_alive") return;

  const cancelId = parseControlCancelId(rec);
  if (cancelId) {
    for (const [uiId, pending] of live.approvals) {
      if (pending.requestId === cancelId) {
        pending.resolve("cancelled");
        live.approvals.delete(uiId);
      }
    }
    for (const [uiId, pending] of live.questions) {
      if (pending.requestId === cancelId) {
        pending.resolve("cancelled");
        live.questions.delete(uiId);
      }
    }
    return;
  }

  const control = parseControlRequest(rec);
  if (control) {
    void handleControlRequest(sessionId, live, control);
    return;
  }

  if (live.muteUpdates) return;

  const sessionIdFromLine = sessionIdFromMessage(rec);
  if (sessionIdFromLine && sessionIdFromLine !== live.claudeSessionId) {
    live.claudeSessionId = sessionIdFromLine;
    resumeByThread.set(sessionId, {
      sessionId: sessionIdFromLine,
      cwd: live.cwd,
    });
    live.onEvent({
      type: "session.providerBound",
      providerSessionId: sessionIdFromLine,
    });
  }

  if (
    type === "system" &&
    (stringField(rec, "subtype") === "init" ||
      stringField(rec, "subtype") === "initialized")
  ) {
    markInitialized(live);
  }

  if (type === "control_response") {
    markInitialized(live);
    return;
  }

  if (live.manualCompaction && type !== "system" && type !== "result") {
    return;
  }

  if (handleAgentLifecycle(live, rec)) return;
  if (type === "tool_progress") {
    handleToolProgress(live, rec);
    return;
  }
  if (type === "stream_event") {
    handleStreamEvent(live, rec);
    return;
  }
  if (type === "assistant") {
    handleAssistant(live, rec);
    return;
  }
  if (type === "user") {
    handleUser(live, rec);
    return;
  }
  if (type === "result") {
    handleResult(live, rec);
    return;
  }
  if (type === "system") {
    const text = statusTextFromSystem(rec);
    if (text) {
      if ((stringField(rec, "subtype") ?? "").startsWith("compact")) {
        live.compactionConfirmed = true;
      }
      live.onEvent({ type: "status", text });
    }
  }
}

/**
 * A line from a subagent carries `parent_tool_use_id`. Its events nest under
 * that Agent call instead of landing in the parent transcript; a line whose
 * parent is not a known Agent call is dropped, as before.
 */
function targetFor(
  live: Live,
  rec: Record<string, unknown>,
): StreamTarget | null {
  const parentId = stringField(rec, "parent_tool_use_id");
  if (!parentId) return { scope: live, emit: live.onEvent };
  const parent = live.toolsById.get(parentId);
  if (!parent || !isAgentToolName(parent.name)) return null;
  // A line after the call settled would reopen a stream nothing will close.
  if (live.finishedAgents.has(parentId)) return null;
  let scope = live.subagents.get(parentId);
  if (!scope) {
    scope = {
      toolsByIndex: new Map(),
      toolsById: new Map(),
      emittedAssistant: "",
      emittedReasoning: "",
    };
    live.subagents.set(parentId, scope);
  }
  return {
    scope,
    parentId,
    emit: (event) =>
      live.onEvent({ type: "subagent.event", callId: parentId, event }),
  };
}

/**
 * `note` tells the parent row what its subagent is up to. A content_block_start
 * has no input yet, so its title is just the tool name; the input delta that
 * follows moments later carries the real one, and notes it then.
 */
function startTool(
  live: Live,
  target: StreamTarget,
  tool: InFlightTool,
  note = true,
): void {
  target.emit({
    type: "tool.started",
    callId: tool.id,
    title: tool.title,
    kind: toolKindFromName(tool.name),
    status: isAgentToolName(tool.name) ? "in_progress" : "pending",
    preview: previewFromTool(tool.name, tool.input),
  });
  if (target.parentId) {
    if (note) noteSubagentTool(live, target.parentId, tool.title);
    return;
  }
  emitTaskListIfNeeded(live, tool.name, tool.input);
  emitSubagentMeta(live, tool);
}

function handleStreamEvent(live: Live, rec: Record<string, unknown>): void {
  const target = targetFor(live, rec);
  if (!target) return;
  const { scope, emit, parentId } = target;
  const delta = streamDeltaFromEvent(rec);
  if (delta) {
    if (delta.kind === "assistant") {
      scope.emittedAssistant = joinStreamText(
        scope.emittedAssistant,
        delta.text,
      );
      emit({ type: "message.delta", text: delta.text });
    } else {
      scope.emittedReasoning = joinStreamText(
        scope.emittedReasoning,
        delta.text,
      );
      emit({ type: "reasoning.delta", text: delta.text });
    }
    return;
  }

  const started = toolStartFromEvent(rec);
  if (started) {
    const tool: InFlightTool = {
      id: started.id,
      name: started.name,
      input: started.input,
      partialJson: "",
      title: toolTitle(started.name, started.input),
    };
    if (started.index >= 0) scope.toolsByIndex.set(started.index, tool);
    scope.toolsById.set(started.id, tool);
    startTool(live, target, tool, false);
    return;
  }

  const jsonDelta = inputJsonDeltaFromEvent(rec);
  if (jsonDelta) {
    const tool = scope.toolsByIndex.get(jsonDelta.index);
    if (!tool) return;
    tool.partialJson += jsonDelta.partial;
    const parsed = tryParseJsonRecord(tool.partialJson);
    if (!parsed) return;
    tool.input = parsed;
    tool.title = toolTitle(tool.name, parsed);
    emit({
      type: "tool.updated",
      callId: tool.id,
      title: tool.title,
      kind: toolKindFromName(tool.name),
      status: "pending",
      detail: summarizeToolRequest(tool.name, parsed),
      preview: previewFromTool(tool.name, parsed),
    });
    if (parentId) {
      noteSubagentTool(live, parentId, tool.title);
      return;
    }
    emitTaskListIfNeeded(live, tool.name, parsed);
    emitSubagentMeta(live, tool);
    return;
  }
}

function handleAssistant(live: Live, rec: Record<string, unknown>): void {
  const target = targetFor(live, rec);
  if (!target) return;
  const { scope, emit, parentId } = target;

  if (parentId) {
    const model = assistantModel(rec);
    if (model) emitSubagentUpdated(live, parentId, { model });
  } else {
    const used = contextUsedFromAssistant(rec);
    if (used !== undefined) live.onEvent({ type: "context", used });
  }

  const snapshot = assistantTextBlocks(rec).join("");
  const extra = snapshotRemainder(scope.emittedAssistant, snapshot);
  if (extra) {
    scope.emittedAssistant = joinStreamText(scope.emittedAssistant, extra);
    emit({ type: "message.delta", text: extra });
  }

  for (const use of assistantToolUses(rec)) {
    if (scope.toolsById.has(use.id)) continue;
    const tool: InFlightTool = {
      id: use.id,
      name: use.name,
      input: use.input,
      partialJson: "",
      title: toolTitle(use.name, use.input),
    };
    scope.toolsById.set(use.id, tool);
    startTool(live, target, tool);
    if (!parentId && use.name === "ExitPlanMode") {
      const plan = extractExitPlanModePlan(use.input);
      if (plan) live.onEvent({ type: "plan", text: plan });
    }
  }
}

function handleUser(live: Live, rec: Record<string, unknown>): void {
  const target = targetFor(live, rec);
  if (!target) return;
  const { scope, emit, parentId } = target;
  for (const result of toolResultsFromUserMessage(rec)) {
    const tool = scope.toolsById.get(result.toolUseId);
    if (!tool) continue;
    const agent = !parentId && isAgentToolName(tool.name);
    // An Agent call's result is not its answer while the task behind it is
    // still running, or when it is just the receipt for an asynchronous
    // launch. The answer lands with the task's own completion.
    if (
      agent &&
      (isBackgroundedAgentTool(live, tool.id) ||
        hasLiveAgentTask(live, tool.id) ||
        isAsyncAgentLaunch(result.text))
    ) {
      live.asyncAgentTools.add(tool.id);
      emitSubagentUpdated(live, tool.id, { background: true });
      continue;
    }
    emit({
      type: "tool.updated",
      callId: tool.id,
      title: tool.title,
      kind: toolKindFromName(tool.name),
      status: result.isError ? "failed" : "completed",
      detail: result.text || undefined,
      preview: previewFromTool(tool.name, tool.input, result.text),
    });
    if (agent) {
      live.subagents.delete(tool.id);
      live.finishedAgents.add(tool.id);
    }
  }
}

function handleResult(live: Live, rec: Record<string, unknown>): void {
  if (isSubagentMessage(rec)) return;
  // A /compact result reports the summarizer call's usage, not the rebuilt
  // conversation level. The next real turn will provide the fresh reading.
  if (!live.manualCompaction) {
    const context = contextFromResult(rec);
    if (context) live.onEvent({ type: "context", ...context });
  }

  const result = turnStatusFromResult(rec);
  if (result.status === "failed" && result.error && !live.cancelled) {
    live.onEvent({ type: "session.error", message: result.error });
  }
  live.turnResultSeen = true;
  maybeFinishTurn(live);
}

async function handleControlRequest(
  sessionId: string,
  live: Live,
  control: ClaudeControlRequest,
): Promise<void> {
  if (control.subtype !== "can_use_tool" && control.subtype !== "permission") {
    await writeJson(
      sessionId,
      buildControlResponse(control.requestId, {}),
    ).catch(() => undefined);
    return;
  }

  const toolName = control.toolName ?? "tool";
  const input = control.input ?? {};

  if (live.cancelled || live.muteUpdates) {
    await writeJson(
      sessionId,
      buildControlResponse(
        control.requestId,
        toClaudePermissionResult("deny", input),
      ),
    ).catch(() => undefined);
    return;
  }

  if (toolName === "AskUserQuestion") {
    const questions = questionsFromUnknown(input);
    const uiId = live.nextApprovalUiId++;
    live.onEvent({
      type: "question.asked",
      requestId: uiId,
      title:
        questionPromptTitle(questions) || extractAskUserQuestionTitle(input),
      questions,
      callId: control.toolUseId,
    });
    const outcome = await waitQuestion(live, uiId, control.requestId);
    const decision =
      outcome === "cancelled"
        ? "cancelled"
        : outcome.kind === "answered"
          ? "answered"
          : "skipped";
    live.onEvent({ type: "question.resolved", requestId: uiId, decision });
    if (outcome === "cancelled") return;
    const response =
      outcome.kind === "answered"
        ? {
            behavior: "allow",
            updatedInput: askUserQuestionAllowInput(input, outcome),
          }
        : {
            behavior: "deny",
            message: "User cancelled tool execution.",
          };
    await writeJson(
      sessionId,
      buildControlResponse(control.requestId, response),
    ).catch(() => undefined);
    return;
  }

  if (toolName === "ExitPlanMode") {
    const plan = extractExitPlanModePlan(input);
    if (plan) live.onEvent({ type: "plan", text: plan });
    await writeJson(
      sessionId,
      buildControlResponse(control.requestId, {
        behavior: "deny",
        message:
          "The client captured your proposed plan. Stop here and wait for the user's feedback or implementation request in a later turn.",
      }),
    ).catch(() => undefined);
    return;
  }

  applyKnownToolInput(live, toolName, input, control.toolUseId);

  if (live.planning) {
    const kind = toolKindFromName(toolName);
    const decision = kind === "read" || kind === "search" ? "allow" : "deny";
    await writeJson(
      sessionId,
      buildControlResponse(
        control.requestId,
        toClaudePermissionResult(decision, input),
      ),
    ).catch(() => undefined);
    return;
  }

  if (live.runtimeMode === "full-access") {
    await writeJson(
      sessionId,
      buildControlResponse(
        control.requestId,
        toClaudePermissionResult("allow", input),
      ),
    ).catch(() => undefined);
    return;
  }

  const uiId = live.nextApprovalUiId++;
  live.onEvent({
    type: "approval.requested",
    requestId: uiId,
    title: toolTitle(toolName, input),
    kind: toolKindFromName(toolName),
    callId: control.toolUseId,
    preview: previewFromTool(toolName, input),
  });
  const decision = await waitApproval(live, uiId, control.requestId, input);
  live.onEvent({ type: "approval.resolved", requestId: uiId, decision });
  if (decision === "cancelled") return;
  await writeJson(
    sessionId,
    buildControlResponse(
      control.requestId,
      toClaudePermissionResult(decision, input),
    ),
  ).catch(() => undefined);
}

function applyKnownToolInput(
  live: Live,
  toolName: string,
  input: Record<string, unknown>,
  callId?: string,
): void {
  if (!callId || Object.keys(input).length === 0) return;
  const update: HarnessEvent = {
    type: "tool.updated",
    callId,
    title: toolTitle(toolName, input),
    kind: toolKindFromName(toolName),
    status: "pending",
    preview: previewFromTool(toolName, input),
  };
  const existing = live.toolsById.get(callId);
  if (existing) {
    existing.input = input;
    existing.title = toolTitle(toolName, input);
    live.onEvent(update);
    return;
  }
  // A subagent's tool asking permission: keep its row inside the subagent,
  // where the call lives, rather than minting a stray one in the parent.
  for (const [parentId, scope] of live.subagents) {
    const nested = scope.toolsById.get(callId);
    if (!nested) continue;
    nested.input = input;
    nested.title = toolTitle(toolName, input);
    live.onEvent({ type: "subagent.event", callId: parentId, event: update });
    return;
  }
  live.onEvent(update);
}

function waitApproval(
  live: Live,
  uiId: number,
  requestId: string,
  input: Record<string, unknown>,
): Promise<ApprovalOutcome> {
  return new Promise((resolve) => {
    live.approvals.set(uiId, { requestId, input, resolve });
  });
}

function waitQuestion(
  live: Live,
  uiId: number,
  requestId: string,
): Promise<UserQuestionReply | "cancelled"> {
  return new Promise((resolve) => {
    live.questions.set(uiId, { requestId, resolve });
  });
}

function emitTaskListIfNeeded(
  live: Live,
  toolName: string,
  input: Record<string, unknown>,
): void {
  if (!isTodoTool(toolName)) return;
  const items = taskListFromTodos(input);
  if (items) live.onEvent({ type: "tasks.updated", items });
}

function handleAgentLifecycle(
  live: Live,
  rec: Record<string, unknown>,
): boolean {
  const started = parseTaskStarted(rec);
  if (started) {
    if (started.ambient || !isAgentTaskType(started.taskType)) return true;
    live.agentTasks.set(started.taskId, {
      taskId: started.taskId,
      toolUseId: started.toolUseId,
      description: started.description,
      backgrounded: started.backgrounded,
    });
    upsertAgentTool(
      live,
      started.toolUseId,
      started.description,
      "in_progress",
      undefined,
      {
        background: started.backgrounded,
        ...(started.subagentType ? { agentType: started.subagentType } : {}),
        ...(started.prompt ? { prompt: started.prompt } : {}),
      },
    );
    return true;
  }

  const progress = parseTaskProgress(rec);
  if (progress) {
    const task = live.agentTasks.get(progress.taskId);
    // The description on a progress line drifts into "Running grep …" as the
    // subagent works. The row keeps the brief it started with; the drift is
    // what it is doing now.
    const title = task?.description || progress.description || "Subagent";
    const activity =
      progress.description && progress.description !== title
        ? progress.description
        : undefined;
    const detail =
      progress.summary ||
      activity ||
      progress.lastToolName ||
      (progress.subagentType
        ? `${progress.subagentType.replace(/[_-]+/g, " ")} subagent`
        : undefined);
    upsertAgentTool(
      live,
      progress.toolUseId ?? task?.toolUseId,
      title,
      "in_progress",
      detail,
      progress.subagentType ? { agentType: progress.subagentType } : undefined,
    );
    return true;
  }

  const updated = parseTaskUpdated(rec);
  if (updated) {
    const task = live.agentTasks.get(updated.taskId);
    if (task && updated.backgrounded !== undefined) {
      task.backgrounded = updated.backgrounded;
      emitSubagentUpdated(live, agentToolId(task.toolUseId, task.description), {
        background: updated.backgrounded,
      });
    }
    if (task && updated.description) task.description = updated.description;
    if (isTerminalAgentTaskStatus(updated.status)) {
      completeAgentTask(
        live,
        updated.taskId,
        updated.status === "completed" ? "completed" : "failed",
        updated.error,
      );
    }
    return true;
  }

  const notice = parseTaskNotification(rec);
  if (notice) {
    if (!notice.ambient) {
      completeAgentTask(
        live,
        notice.taskId,
        notice.status === "completed" ? "completed" : "failed",
        notice.summary || undefined,
      );
    }
    return true;
  }

  const liveTasks = parseBackgroundAgentTasks(rec);
  if (!liveTasks) return false;
  const next = new Set(liveTasks.map((task) => task.taskId));
  for (const id of [...live.agentTasks.keys()]) {
    if (!next.has(id)) completeAgentTask(live, id, "completed");
  }
  // A launch receipt with no task behind it any more is finished too;
  // otherwise the turn would wait forever on an agent the CLI has forgotten.
  for (const id of [...live.asyncAgentTools]) {
    if (!hasLiveAgentTask(live, id)) live.asyncAgentTools.delete(id);
  }
  for (const row of liveTasks) {
    if (live.agentTasks.has(row.taskId)) continue;
    // This list can land a line before `task_started`. Remember the task so
    // the turn stays open, but only give it a row if an Agent call already
    // has one; minting a row here left a stray duplicate with no transcript
    // once `task_started` pointed at the real call.
    const toolUseId = agentToolIdByTitle(live, row.description);
    live.agentTasks.set(row.taskId, {
      taskId: row.taskId,
      toolUseId,
      description: row.description,
      backgrounded: true,
    });
    if (toolUseId) {
      upsertAgentTool(
        live,
        toolUseId,
        row.description,
        "in_progress",
        undefined,
        {
          background: true,
        },
      );
    }
  }
  maybeFinishTurn(live);
  return true;
}

function handleToolProgress(live: Live, rec: Record<string, unknown>): void {
  const progress = parseToolProgress(rec);
  if (!progress) return;
  const tool =
    live.toolsById.get(progress.toolUseId) ??
    (progress.parentToolUseId
      ? live.toolsById.get(progress.parentToolUseId)
      : undefined);
  if (!tool || !isAgentToolName(tool.name)) return;
  const detail = progress.subagentType
    ? `${progress.subagentType.replace(/[_-]+/g, " ")} subagent`
    : progress.toolName;
  live.onEvent({
    type: "tool.updated",
    callId: tool.id,
    title: tool.title,
    kind: "agent",
    status: "in_progress",
    ...(detail ? { detail } : {}),
  });
  if (progress.subagentType) {
    emitSubagentUpdated(live, tool.id, { agentType: progress.subagentType });
  }
}

/** The parent's Agent row shows the subagent's latest step as its detail. */
function noteSubagentTool(live: Live, parentId: string, step: string): void {
  const parent = live.toolsById.get(parentId);
  if (!parent || !isAgentToolName(parent.name)) return;
  live.onEvent({
    type: "tool.updated",
    callId: parent.id,
    title: parent.title,
    kind: "agent",
    status: "in_progress",
    detail: step,
  });
}

/**
 * Tell the transcript something about a subagent, once. The CLI repeats the
 * model on every message and the type on every progress line; each repeat
 * would otherwise cost a full reducer pass for nothing.
 */
function emitSubagentUpdated(
  live: Live,
  callId: string,
  patch: SubagentMetaPatch,
): void {
  const sent = live.subagentMeta.get(callId) ?? {};
  const changed: SubagentMetaPatch = {};
  for (const key of SUBAGENT_META_KEYS) {
    const value = patch[key];
    if (value === undefined || value === "" || value === sent[key]) continue;
    Object.assign(changed, { [key]: value });
  }
  if (Object.keys(changed).length === 0) return;
  live.subagentMeta.set(callId, { ...sent, ...changed });
  live.onEvent({ type: "subagent.updated", callId, ...changed });
}

/** What the Agent call's own input says about the subagent it spawns. */
function emitSubagentMeta(live: Live, tool: InFlightTool): void {
  if (!isAgentToolName(tool.name)) return;
  const meta = subagentMetaFromInput(tool.input);
  if (meta) emitSubagentUpdated(live, tool.id, meta);
}

function agentToolId(callId: string | undefined, title: string): string {
  return callId ?? `agent:${title}`;
}

/** The Agent call already on the transcript with this brief, if there is one. */
function agentToolIdByTitle(live: Live, title: string): string | undefined {
  for (const tool of live.toolsById.values()) {
    if (isAgentToolName(tool.name) && tool.title === title) return tool.id;
  }
  return undefined;
}

function hasLiveAgentTask(live: Live, toolUseId: string): boolean {
  for (const task of live.agentTasks.values()) {
    if (task.toolUseId === toolUseId) return true;
  }
  return false;
}

function isBackgroundedAgentTool(live: Live, toolUseId: string): boolean {
  for (const task of live.agentTasks.values()) {
    if (task.toolUseId === toolUseId && task.backgrounded) return true;
  }
  return false;
}

function upsertAgentTool(
  live: Live,
  callId: string | undefined,
  title: string,
  status: string,
  detail?: string,
  meta?: SubagentMetaPatch,
): string {
  const id = agentToolId(callId ?? agentToolIdByTitle(live, title), title);
  const existing = live.toolsById.get(id);
  const emitMeta = () => {
    if (meta) emitSubagentUpdated(live, id, meta);
  };
  if (!existing) {
    live.toolsById.set(id, {
      id,
      name: "Agent",
      input: {},
      partialJson: "",
      title,
    });
    live.onEvent({
      type: "tool.started",
      callId: id,
      title,
      kind: "agent",
      status,
    });
    emitMeta();
    if (
      status !== "in_progress" &&
      status !== "pending" &&
      status !== "running"
    ) {
      live.onEvent({
        type: "tool.updated",
        callId: id,
        title,
        kind: "agent",
        status,
        ...(detail ? { detail } : {}),
      });
    }
    return id;
  }
  if (title) existing.title = title;
  emitMeta();
  live.onEvent({
    type: "tool.updated",
    callId: id,
    title: existing.title,
    kind: "agent",
    status,
    ...(detail ? { detail } : {}),
  });
  return id;
}

function completeAgentTask(
  live: Live,
  taskId: string,
  status: string,
  detail?: string,
): void {
  const task = live.agentTasks.get(taskId);
  live.agentTasks.delete(taskId);
  if (task) {
    const id = upsertAgentTool(
      live,
      task.toolUseId,
      task.description,
      status,
      detail,
    );
    live.subagents.delete(id);
    live.asyncAgentTools.delete(id);
    live.finishedAgents.add(id);
  }
  maybeFinishTurn(live);
}

function maybeFinishTurn(live: Live): void {
  if (!live.turnResultSeen) return;
  if (live.agentTasks.size > 0 || live.asyncAgentTools.size > 0) return;
  if (!live.activeTurn && !live.turnDone) return;
  finishActiveTurn(live, [
    { type: "message.completed" },
    { type: "reasoning.completed" },
  ]);
}

function finishActiveTurn(live: Live, extraEvents: HarnessEvent[] = []): void {
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

function markInitialized(live: Live): void {
  if (live.initialized) return;
  live.initialized = true;
  live.initDone?.();
  live.initDone = null;
}

function waitForInit(live: Live, timeoutMs: number): Promise<void> {
  if (live.initialized) return Promise.resolve();
  return new Promise((resolve) => {
    const timer = setTimeout(() => {
      live.initDone = null;
      resolve();
    }, timeoutMs);
    live.initDone = () => {
      clearTimeout(timer);
      resolve();
    };
  });
}

function nextControlId(live: Live): string {
  live.nextControlId += 1;
  return `monocode_${live.nextControlId}`;
}

function writeJson(
  sessionId: string,
  payload: Record<string, unknown>,
): Promise<void> {
  return writeChild(sessionId, JSON.stringify(payload));
}

function settingsKeyFor(input: HarnessSessionInput): string {
  return claudeSettingsKey({
    model: nativeModelId(input.model),
    effort: input.modelSettings?.effort,
    fast: input.modelSettings?.fast,
    thinking: input.modelSettings?.thinking,
    context: input.modelSettings?.context,
    runtimeMode: input.runtimeMode,
    hooks: loadClaudeHooks(),
  });
}

function launchOptions(
  input: HarnessSessionInput,
  resume: string | undefined,
  sessionId: string,
): {
  model?: string;
  effort?: string;
  permissionMode?: ReturnType<typeof runtimeModeToPermission>;
  resume?: string;
  sessionId?: string;
  settings?: ClaudeCliSettings;
} {
  const native = nativeModelId(input.model);
  const effortRaw = input.modelSettings?.effort;
  const context = input.modelSettings?.context;
  const settings: ClaudeCliSettings = {};
  if (input.modelSettings?.thinking === "true") {
    settings.alwaysThinkingEnabled = true;
  }
  if (input.modelSettings?.fast === "true") {
    settings.fastMode = true;
  }
  if (isClaudeUltracodeEffort(effortRaw)) {
    settings.ultracode = true;
  }
  if (!loadClaudeHooks()) {
    settings.disableAllHooks = true;
  }
  return {
    model: resolveClaudeApiModelId(native, context),
    effort: normalizeClaudeCliEffort(effortRaw, native),
    permissionMode:
      input.intent === "plan"
        ? "plan"
        : runtimeModeToPermission(input.runtimeMode),
    resume,
    sessionId: resume ? undefined : sessionId,
    settings: Object.keys(settings).length > 0 ? settings : undefined,
  };
}

/** Exported for tests. */
export function __claudeTestReset(): void {
  liveByThread.clear();
  resumeByThread.clear();
  cancelledThreads.clear();
}
