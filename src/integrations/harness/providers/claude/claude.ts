import { nativeModelId } from "../../../../features/sessions/model/models";
import { sameProviderAccountId } from "../../../../features/providers/model/providerAccounts";
import type { RuntimeMode } from "../../../../features/sessions/model/session";
import { loadClaudeHooks } from "../../../../features/settings/model/settings";
import {
  killChild,
  resolveClaudeBinary,
  spawnChild,
  unwatchChild,
  watchChild,
  writeChild,
} from "../../core/child";
import {
  askUserQuestionAllowInput,
  asRecord,
  assistantMessageId,
  assistantTextBlocks,
  assistantThinkingBlocks,
  assistantToolUses,
  contextFromResult,
  contextUsedFromAssistant,
  turnMetricsFromResult,
  buildClaudeSpawnArgs,
  buildClaudeUserMessage,
  buildControlRequest,
  buildControlResponse,
  claudeSettingsKey,
  extractAskUserQuestionTitle,
  extractExitPlanModePlan,
  inputJsonDeltaFromEvent,
  isAgentTaskType,
  isClaudeUltracodeEffort,
  isSubagentMessage,
  isTerminalAgentTaskStatus,
  isTodoTool,
  isUsageLimitResult,
  normalizeClaudeCliEffort,
  parseBackgroundTasks,
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
  usageLimitFromRateLimitEvent,
  type ClaudeAgentTaskNotification,
  type ClaudeCliSettings,
  type ClaudeControlRequest,
} from "./claudeProtocol";
import { isAgentToolName } from "../../core/preview";
import { joinStreamText, snapshotRemainder } from "../../core/streamText";
import {
  questionPromptTitle,
  questionsFromUnknown,
  type UserQuestionReply,
} from "../../../../features/sessions/model/userQuestion";
import type {
  ApprovalDecision,
  CompactContextInput,
  HarnessEvent,
  HarnessSessionInput,
  SendTurnInput,
  SteerTurnInput,
} from "../../core/types";

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
  event: Extract<HarnessEvent, { type: "question.asked" }>;
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

type BackgroundTask = {
  description: string;
  toolUseId?: string;
};

type Live = {
  cwd: string;
  claudeSessionId: string;
  providerAccountId?: string;
  runtimeMode: RuntimeMode;
  planning: boolean;
  settingsKey: string;
  onEvent: (event: HarnessEvent) => void;
  approvals: Map<number, PendingApproval>;
  questions: Map<number, PendingQuestion>;
  visibleQuestionId: number | null;
  nextApprovalUiId: number;
  nextControlId: number;
  toolsByIndex: Map<number, InFlightTool>;
  toolsById: Map<string, InFlightTool>;
  agentTasks: Map<string, LiveAgentTask>;
  /**
   * Every task Claude still runs for this session, by id: subagents, shells it
   * backgrounded, monitors. Each one ends in a notification that wakes Claude
   * for another turn, so the MonoCode turn stays open until they are done.
   */
  backgroundTasks: Map<string, BackgroundTask>;
  /** Rows shown for tasks still running when Claude yielded, by task id. */
  backgroundRows: Map<string, string>;
  /** A task finished after Claude yielded; its follow-up turn is on the way. */
  awaitingResume: ReturnType<typeof setTimeout> | null;
  /** Last background list sent to the UI, to skip repeats. */
  backgroundKey: string;
  /** Finished-subagent notes held until Claude picks the thread back up. */
  taskNotes: string[];
  turnResultSeen: boolean;
  /** Latest `rate_limit_event` refused requests; reported when the turn ends. */
  usageLimit: { resetsAt?: number } | null;
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
  pendingAssistantBoundary: boolean;
  manualCompaction: boolean;
  compactionConfirmed: boolean;
};

type Resume = {
  sessionId: string;
  cwd: string;
  providerAccountId?: string;
};

const INIT_TIMEOUT_MS = 8_000;
/**
 * How long a finished background task may take to wake Claude before the turn
 * is let go anyway. The follow-up turn normally starts within a second or two.
 */
const RESUME_GRACE_MS = 15_000;

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
  // Stop means the whole run, including what Claude left going in the
  // background. Otherwise it finishes later and wakes Claude up again.
  for (const taskId of live.backgroundTasks.keys()) {
    await writeJson(
      sessionId,
      buildControlRequest(nextControlId(live), {
        subtype: "stop_task",
        task_id: taskId,
      }),
    ).catch(() => undefined);
  }
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
  providerAccountId?: string,
): void {
  const sessionId = providerSessionId.trim();
  if (!threadId || !sessionId || !cwd.trim()) return;
  resumeByThread.set(threadId, { sessionId, cwd, providerAccountId });
}

/**
 * Records in a stored session file that carry conversation content. The rest —
 * modes, file snapshots, queue operations, generated titles — is bookkeeping
 * the transcript never showed in the first place.
 */
function isReplayableRecord(rec: Record<string, unknown>): boolean {
  const type = rec.type;
  if (type !== "user" && type !== "assistant") return false;
  // Subagent traffic is already represented by the Agent tool row on the
  // parent message; replaying it too would duplicate the work inline.
  return rec.isSidechain !== true;
}

/**
 * The prompt a person typed, or nothing when this record is a tool result
 * wearing the user role — those are answers to the agent's own calls and are
 * replayed through `handleLine` onto their tool rows instead.
 */
function userPromptText(rec: Record<string, unknown>): string | undefined {
  const message = rec.message as Record<string, unknown> | undefined;
  const content = message?.content;
  if (typeof content === "string") return content.trim() || undefined;
  if (!Array.isArray(content)) return undefined;
  const blocks = content as Array<Record<string, unknown>>;
  if (blocks.some((block) => block?.type === "tool_result")) return undefined;
  const text = blocks
    .filter((block) => block?.type === "text")
    .map((block) => (typeof block.text === "string" ? block.text : ""))
    .join("\n")
    .trim();
  return text || undefined;
}

/**
 * Rebuild a thread's transcript from a conversation Claude stored on disk.
 *
 * The stored records are the same shape as the ones the live process streams,
 * so they go through `handleLine` unchanged and produce the same events —
 * including tool rows and subagent steps. Passing the MonoCode thread id also
 * means `handleLine` records the resume binding on its own, from `cwd`, so that
 * has to be the directory the next turn will run in: the working copy, not the
 * project root a worktree session is filed under.
 */
export function replayClaudeSession(input: {
  sessionId: string;
  /** Directory the next turn runs in; the binding replay records is keyed to it. */
  cwd: string;
  providerAccountId?: string;
  runtimeMode: RuntimeMode;
  transcript: string;
  onEvent: (event: HarnessEvent) => void;
  /** A prompt someone typed. These are blocks, not events, so the caller adds
   * them to the transcript itself. */
  onPrompt: (text: string, at?: number) => void;
}): void {
  // Two things can leave a reply open. `handleLine` holds its own boundary for
  // a message carrying text, but a later tool-only record overwrites it; and
  // live the final reply is closed by the turn's `result` message, which a
  // stored conversation has no equivalent of. Watching what was actually
  // emitted covers both without closing the same message twice.
  let openReply = false;
  const emit = (event: HarnessEvent) => {
    if (event.type === "message.delta") openReply = true;
    if (event.type === "message.completed") openReply = false;
    input.onEvent(event);
  };

  const live = newLiveState({
    cwd: input.cwd,
    claudeSessionId: "",
    providerAccountId: input.providerAccountId,
    runtimeMode: input.runtimeMode,
    planning: false,
    settingsKey: "",
    onEvent: emit,
  });
  live.initialized = true;

  const finishReply = () => {
    closePendingAssistantMessage(live);
    if (openReply) emit({ type: "message.completed" });
  };

  for (const line of input.transcript.split("\n")) {
    const trimmed = line.trim();
    if (!trimmed) continue;
    let rec: Record<string, unknown>;
    try {
      rec = JSON.parse(trimmed) as Record<string, unknown>;
    } catch {
      continue;
    }
    if (!isReplayableRecord(rec)) continue;

    const prompt = rec.type === "user" ? userPromptText(rec) : undefined;
    if (prompt) {
      // End the reply this prompt follows, so the turns stay separate blocks.
      finishReply();
      input.onPrompt(prompt, timestampOf(rec));
      continue;
    }

    handleLine(input.sessionId, live, trimmed);
  }
  finishReply();
}

function timestampOf(rec: Record<string, unknown>): number | undefined {
  const raw = rec.timestamp;
  if (typeof raw !== "string") return undefined;
  const parsed = Date.parse(raw);
  return Number.isNaN(parsed) ? undefined : parsed;
}

/**
 * Every field of a fresh `Live`. Shared so that replaying a stored transcript
 * runs against exactly the same state the live stream builds, rather than a
 * second copy that drifts as the streaming path grows.
 */
function newLiveState(input: {
  cwd: string;
  claudeSessionId: string;
  providerAccountId?: string;
  runtimeMode: RuntimeMode;
  planning: boolean;
  settingsKey: string;
  onEvent: (event: HarnessEvent) => void;
}): Live {
  return {
    cwd: input.cwd,
    claudeSessionId: input.claudeSessionId,
    providerAccountId: input.providerAccountId,
    runtimeMode: input.runtimeMode,
    planning: input.planning,
    settingsKey: input.settingsKey,
    onEvent: input.onEvent,
    approvals: new Map(),
    questions: new Map(),
    visibleQuestionId: null,
    nextApprovalUiId: 1,
    nextControlId: 1,
    toolsByIndex: new Map(),
    toolsById: new Map(),
    agentTasks: new Map(),
    backgroundTasks: new Map(),
    backgroundRows: new Map(),
    awaitingResume: null,
    backgroundKey: "",
    taskNotes: [],
    turnResultSeen: false,
    usageLimit: null,
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
    pendingAssistantBoundary: false,
    manualCompaction: false,
    compactionConfirmed: false,
  };
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
    // Model and launch-setting changes require a fresh Claude process, but
    // they must resume the same provider conversation. Only a cwd change
    // invalidates the stored session because Claude sessions are cwd-bound.
    if (existing.cwd !== input.cwd) resumeByThread.delete(input.sessionId);
    await stopClaudeSession(input.sessionId);
  }

  const resume = resumeByThread.get(input.sessionId);
  const canResume =
    resume != null &&
    resume.cwd === input.cwd &&
    sameProviderAccountId(resume.providerAccountId, input.providerAccountId);
  if (
    resume &&
    (resume.cwd !== input.cwd ||
      !sameProviderAccountId(resume.providerAccountId, input.providerAccountId))
  ) {
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

  const live: Live = newLiveState({
    cwd: input.cwd,
    claudeSessionId,
    providerAccountId: input.providerAccountId,
    runtimeMode: input.runtimeMode,
    planning,
    settingsKey,
    onEvent: input.onEvent,
  });
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
      const current = liveRef.current;
      if (!current?.muteUpdates) {
        (current?.onEvent ?? input.onEvent)({ type: "session.ended", code });
      }
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
    { provider: "claude", id: input.providerAccountId ?? "default" },
  );

  liveByThread.set(input.sessionId, live);
  resumeByThread.set(input.sessionId, {
    sessionId: claudeSessionId,
    cwd: input.cwd,
    providerAccountId: input.providerAccountId,
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
  live.pendingAssistantBoundary = false;
  live.toolsByIndex.clear();
  live.toolsById.clear();
  live.agentTasks.clear();
  live.backgroundTasks.clear();
  live.backgroundRows.clear();
  clearAwaitingResume(live);
  live.backgroundKey = "";
  live.taskNotes = [];
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
    const turn = live.turnDone;
    void handleControlRequest(sessionId, live, control).catch(
      (error: unknown) => {
        if (live.muteUpdates || live.turnDone !== turn) return;
        const failure =
          error instanceof Error ? error : new Error(String(error));
        if (live.turnFailed) {
          live.turnFailed(failure);
        } else {
          live.onEvent({ type: "session.error", message: failure.message });
        }
      },
    );
    return;
  }

  if (live.muteUpdates) return;

  const sessionIdFromLine = sessionIdFromMessage(rec);
  if (sessionIdFromLine && sessionIdFromLine !== live.claudeSessionId) {
    live.claudeSessionId = sessionIdFromLine;
    resumeByThread.set(sessionId, {
      sessionId: sessionIdFromLine,
      cwd: live.cwd,
      providerAccountId: live.providerAccountId,
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
    if (stringField(rec, "subtype") === "init") noteClaudeTurnStarted(live);
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
    if (!isSubagentMessage(rec)) noteClaudeTurnStarted(live);
    handleStreamEvent(live, rec);
    return;
  }
  if (type === "assistant") {
    if (!isSubagentMessage(rec)) noteClaudeTurnStarted(live);
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
  if (type === "rate_limit_event") {
    live.usageLimit = usageLimitFromRateLimitEvent(rec);
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

function handleStreamEvent(live: Live, rec: Record<string, unknown>): void {
  const subagent = isSubagentMessage(rec);
  const delta = streamDeltaFromEvent(rec);
  if (delta) {
    if (subagent) return;
    if (delta.kind === "assistant") {
      closePendingAssistantMessage(live);
      live.emittedAssistant = joinStreamText(live.emittedAssistant, delta.text);
      live.onEvent({ type: "message.delta", text: delta.text });
    } else {
      live.emittedReasoning = joinStreamText(live.emittedReasoning, delta.text);
      live.onEvent({ type: "reasoning.delta", text: delta.text });
    }
    return;
  }

  const started = toolStartFromEvent(rec);
  if (started) {
    if (subagent) {
      noteSubagentTool(live, rec, started.id, started.name, started.input);
      return;
    }
    const tool: InFlightTool = {
      id: started.id,
      name: started.name,
      input: started.input,
      partialJson: "",
      title: toolTitle(started.name, started.input),
    };
    if (started.index >= 0) live.toolsByIndex.set(started.index, tool);
    live.toolsById.set(started.id, tool);
    live.onEvent({
      type: "tool.started",
      callId: tool.id,
      title: tool.title,
      kind: toolKindFromName(tool.name),
      ...(isAgentToolName(tool.name) && stringField(tool.input, "model")
        ? { agentModel: stringField(tool.input, "model") }
        : {}),
      status: isAgentToolName(tool.name) ? "in_progress" : "pending",
      preview: previewFromTool(tool.name, tool.input),
    });
    emitTaskListIfNeeded(live, tool.name, tool.input);
    return;
  }

  const jsonDelta = inputJsonDeltaFromEvent(rec);
  if (jsonDelta) {
    if (subagent) return;
    const tool = live.toolsByIndex.get(jsonDelta.index);
    if (!tool) return;
    tool.partialJson += jsonDelta.partial;
    const parsed = tryParseJsonRecord(tool.partialJson);
    if (!parsed) return;
    tool.input = parsed;
    tool.title = toolTitle(tool.name, parsed);
    live.onEvent({
      type: "tool.updated",
      callId: tool.id,
      title: tool.title,
      kind: toolKindFromName(tool.name),
      ...(isAgentToolName(tool.name) && stringField(tool.input, "model")
        ? { agentModel: stringField(tool.input, "model") }
        : {}),
      status: "pending",
      detail: summarizeToolRequest(tool.name, parsed),
      preview: previewFromTool(tool.name, parsed),
    });
    emitTaskListIfNeeded(live, tool.name, parsed);
    return;
  }
}

function handleAssistant(live: Live, rec: Record<string, unknown>): void {
  if (isSubagentMessage(rec)) {
    noteSubagentNarration(live, rec);
    for (const use of assistantToolUses(rec)) {
      noteSubagentTool(live, rec, use.id, use.name, use.input);
    }
    return;
  }

  const used = contextUsedFromAssistant(rec);
  if (used !== undefined) live.onEvent({ type: "context", used });

  const snapshot = assistantTextBlocks(rec).join("");
  if (snapshot) closePendingAssistantMessage(live);
  const extra = snapshotRemainder(live.emittedAssistant, snapshot);
  if (extra) {
    live.emittedAssistant = joinStreamText(live.emittedAssistant, extra);
    live.onEvent({ type: "message.delta", text: extra });
  }

  for (const use of assistantToolUses(rec)) {
    const streamed = live.toolsById.get(use.id);
    if (streamed) {
      // content_block_start often has an empty input. The input JSON delta may
      // never form a parseable object before the complete assistant snapshot.
      // Reconcile that snapshot instead of leaving the tool labelled "Shell".
      if (JSON.stringify(streamed.input) !== JSON.stringify(use.input)) {
        streamed.input = use.input;
        streamed.title = toolTitle(use.name, use.input);
        live.onEvent({
          type: "tool.updated",
          callId: streamed.id,
          title: streamed.title,
          kind: toolKindFromName(streamed.name),
          ...(isAgentToolName(streamed.name) && stringField(use.input, "model")
            ? { agentModel: stringField(use.input, "model") }
            : {}),
          status: isAgentToolName(streamed.name) ? "in_progress" : "pending",
          preview: previewFromTool(streamed.name, use.input),
        });
        emitTaskListIfNeeded(live, streamed.name, use.input);
      }
      if (use.name === "ExitPlanMode") {
        const plan = extractExitPlanModePlan(use.input);
        if (plan) live.onEvent({ type: "plan", text: plan });
      }
      continue;
    }
    const tool: InFlightTool = {
      id: use.id,
      name: use.name,
      input: use.input,
      partialJson: "",
      title: toolTitle(use.name, use.input),
    };
    live.toolsById.set(use.id, tool);
    live.onEvent({
      type: "tool.started",
      callId: tool.id,
      title: tool.title,
      kind: toolKindFromName(tool.name),
      ...(isAgentToolName(tool.name) && stringField(tool.input, "model")
        ? { agentModel: stringField(tool.input, "model") }
        : {}),
      status: isAgentToolName(tool.name) ? "in_progress" : "pending",
      preview: previewFromTool(tool.name, tool.input),
    });
    if (use.name === "ExitPlanMode") {
      const plan = extractExitPlanModePlan(use.input);
      if (plan) live.onEvent({ type: "plan", text: plan });
    }
    emitTaskListIfNeeded(live, tool.name, tool.input);
  }

  // Each assistant record is one Claude message. Wait until the next message
  // begins to close its UI block, so a backgrounded turn stays visibly live.
  live.pendingAssistantBoundary = !!(snapshot || live.emittedAssistant);
  live.emittedAssistant = "";
  live.emittedReasoning = "";
}

function closePendingAssistantMessage(live: Live): void {
  if (!live.pendingAssistantBoundary) return;
  live.pendingAssistantBoundary = false;
  live.onEvent({ type: "message.completed" });
}

function handleUser(live: Live, rec: Record<string, unknown>): void {
  if (isSubagentMessage(rec)) {
    noteSubagentResults(live, rec);
    return;
  }
  for (const result of toolResultsFromUserMessage(rec)) {
    const tool = live.toolsById.get(result.toolUseId);
    if (!tool) continue;
    if (isAgentToolName(tool.name) && isBackgroundedAgentTool(live, tool.id)) {
      continue;
    }
    live.onEvent({
      type: "tool.updated",
      callId: tool.id,
      title: tool.title,
      kind: toolKindFromName(tool.name),
      status: result.isError ? "failed" : "completed",
      detail: result.text || undefined,
      preview: previewFromTool(tool.name, tool.input, result.text),
    });
    // What a subagent hands back is the last thing it said, so it closes out
    // that agent's own trail rather than sitting on the parent row as detail.
    if (isAgentToolName(tool.name) && result.text.trim() && !result.isError) {
      live.onEvent({
        type: "agent.step",
        callId: tool.id,
        stepId: `${tool.id}:report`,
        kind: "message",
        text: result.text,
      });
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
  const metrics = turnMetricsFromResult(rec);
  if (metrics) live.onEvent({ type: "turn.metrics", ...metrics });

  const result = turnStatusFromResult(rec);
  if (result.status === "failed" && result.error && !live.cancelled) {
    live.onEvent({ type: "session.error", message: result.error });
  }
  // A refused window can still fall back to another model, so only a turn
  // that ended in error was stopped by it.
  const turnErrored = rec.is_error === true || result.status === "failed";
  const usageLimit =
    live.usageLimit ?? (isUsageLimitResult(rec) ? {} : null);
  live.usageLimit = null;
  if (usageLimit && turnErrored && !live.cancelled) {
    live.onEvent({ type: "usage.limited", ...usageLimit });
  }
  live.turnResultSeen = true;
  maybeFinishTurn(live);
  showBackgroundRows(live);
  syncBackgroundWait(live);
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
    );
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
    const pending = waitQuestion(live, uiId, control.requestId, {
      type: "question.asked",
      requestId: uiId,
      title:
        questionPromptTitle(questions) || extractAskUserQuestionTitle(input),
      questions,
      callId: control.toolUseId,
    });
    showNextQuestion(live);
    const outcome = await pending;
    const decision =
      outcome === "cancelled"
        ? "cancelled"
        : outcome.kind === "answered"
          ? "answered"
          : "skipped";
    live.onEvent({ type: "question.resolved", requestId: uiId, decision });
    showNextQuestion(live);
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
    );
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
    );
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
    );
    return;
  }

  if (live.runtimeMode === "full-access") {
    await writeJson(
      sessionId,
      buildControlResponse(
        control.requestId,
        toClaudePermissionResult("allow", input),
      ),
    );
    return;
  }

  const uiId = live.nextApprovalUiId++;
  const pending = waitApproval(live, uiId, control.requestId, input);
  live.onEvent({
    type: "approval.requested",
    requestId: uiId,
    title: toolTitle(toolName, input),
    kind: toolKindFromName(toolName),
    callId: control.toolUseId,
    preview: previewFromTool(toolName, input),
  });
  const decision = await pending;
  live.onEvent({ type: "approval.resolved", requestId: uiId, decision });
  if (decision === "cancelled") return;
  await writeJson(
    sessionId,
    buildControlResponse(
      control.requestId,
      toClaudePermissionResult(decision, input),
    ),
  );
}

function applyKnownToolInput(
  live: Live,
  toolName: string,
  input: Record<string, unknown>,
  callId?: string,
): void {
  if (!callId || Object.keys(input).length === 0) return;
  const existing = live.toolsById.get(callId);
  if (existing) {
    existing.input = input;
    existing.title = toolTitle(toolName, input);
  }
  live.onEvent({
    type: "tool.updated",
    callId,
    title: toolTitle(toolName, input),
    kind: toolKindFromName(toolName),
    status: "pending",
    preview: previewFromTool(toolName, input),
  });
}

function waitApproval(
  live: Live,
  uiId: number,
  requestId: string,
  input: Record<string, unknown>,
): Promise<ApprovalOutcome> {
  return new Promise<ApprovalOutcome>((resolve) => {
    live.approvals.set(uiId, { requestId, input, resolve });
  }).finally(() => {
    live.approvals.delete(uiId);
  });
}

function waitQuestion(
  live: Live,
  uiId: number,
  requestId: string,
  event: Extract<HarnessEvent, { type: "question.asked" }>,
): Promise<UserQuestionReply | "cancelled"> {
  return new Promise<UserQuestionReply | "cancelled">((resolve) => {
    live.questions.set(uiId, { requestId, event, resolve });
  }).finally(() => {
    live.questions.delete(uiId);
  });
}

function showNextQuestion(live: Live): void {
  if (live.muteUpdates || live.cancelled) return;
  if (
    live.visibleQuestionId !== null &&
    live.questions.has(live.visibleQuestionId)
  )
    return;
  const next = live.questions.entries().next().value;
  live.visibleQuestionId = next?.[0] ?? null;
  if (next) live.onEvent(next[1].event);
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
    if (started.ambient) return true;
    live.backgroundTasks.set(started.taskId, {
      description: started.description,
      toolUseId: started.toolUseId,
    });
    syncBackgroundWait(live);
    if (!isAgentTaskType(started.taskType)) return true;
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
    );
    return true;
  }

  const progress = parseTaskProgress(rec);
  if (progress) {
    const task = live.agentTasks.get(progress.taskId);
    const title = progress.description || task?.description || "Subagent";
    const detail =
      progress.summary ||
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
    );
    return true;
  }

  const updated = parseTaskUpdated(rec);
  if (updated) {
    const task = live.agentTasks.get(updated.taskId);
    if (task && updated.backgrounded !== undefined) {
      task.backgrounded = updated.backgrounded;
    }
    if (task && updated.description) task.description = updated.description;
    const background = live.backgroundTasks.get(updated.taskId);
    if (background && updated.description) {
      background.description = updated.description;
    }
    if (isTerminalAgentTaskStatus(updated.status)) {
      settleBackgroundRow(
        live,
        updated.taskId,
        updated.status ?? "completed",
        updated.error,
      );
      finishBackgroundTask(live, updated.taskId);
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
      noteTaskNotification(live, notice);
      finishBackgroundTask(live, notice.taskId);
      completeAgentTask(
        live,
        notice.taskId,
        notice.status === "completed" ? "completed" : "failed",
        notice.summary || undefined,
      );
    }
    return true;
  }

  const allTasks = parseBackgroundTasks(rec);
  if (!allTasks) return false;
  const next = new Set(allTasks.map((task) => task.taskId));
  for (const id of [...live.backgroundTasks.keys()]) {
    if (next.has(id)) continue;
    settleBackgroundRow(live, id, "completed");
    finishBackgroundTask(live, id);
  }
  for (const row of allTasks) {
    if (!live.backgroundTasks.has(row.taskId)) {
      live.backgroundTasks.set(row.taskId, { description: row.description });
    }
  }
  const liveTasks = allTasks.filter((task) => isAgentTaskType(task.taskType));
  for (const id of [...live.agentTasks.keys()]) {
    if (!next.has(id)) completeAgentTask(live, id, "completed");
  }
  for (const row of liveTasks) {
    if (live.agentTasks.has(row.taskId)) continue;
    live.agentTasks.set(row.taskId, {
      taskId: row.taskId,
      description: row.description,
      backgrounded: true,
    });
    upsertAgentTool(live, undefined, row.description, "in_progress");
  }
  maybeFinishTurn(live);
  syncBackgroundWait(live);
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
  live.onEvent({
    type: "tool.updated",
    callId: tool.id,
    title: tool.title,
    kind: "agent",
    status: "in_progress",
  });
  // Progress names the call in flight. That is a step in the run, not the
  // result of it, so it goes to the panel rather than onto the Agent row.
  if (progress.toolName) {
    live.onEvent({
      type: "agent.step",
      callId: tool.id,
      stepId: progress.toolUseId,
      kind: "tool",
      text: progress.toolName,
      status: "in_progress",
      ...(progress.subagentType ? { agentType: progress.subagentType } : {}),
    });
  }
}

/**
 * The Agent call a subagent message belongs to, or nothing when the message
 * came from somewhere the parent transcript has no row for.
 */
function subagentParent(
  live: Live,
  rec: Record<string, unknown>,
): InFlightTool | undefined {
  const parentId = stringField(rec, "parent_tool_use_id");
  if (!parentId) return undefined;
  const parent = live.toolsById.get(parentId);
  if (!parent || !isAgentToolName(parent.name)) return undefined;
  return parent;
}

/**
 * A call a subagent made, mirrored onto the Agent row that spawned it. The
 * parent keeps its own "still running" status; the step is what the panel
 * under that row reads back.
 */
function noteSubagentTool(
  live: Live,
  rec: Record<string, unknown>,
  id: string,
  name: string,
  input: Record<string, unknown>,
): void {
  const parent = subagentParent(live, rec);
  if (!parent) return;
  const title = toolTitle(name, input);
  // No detail: the Agent row's detail is the report the run hands back, and
  // writing the call of the moment there would leave whatever the subagent
  // happened to do last standing in as its result.
  live.onEvent({
    type: "tool.updated",
    callId: parent.id,
    title: parent.title,
    kind: "agent",
    status: "in_progress",
  });
  if (!id) return;
  const preview = previewFromTool(name, input);
  live.onEvent({
    type: "agent.step",
    callId: parent.id,
    stepId: id,
    kind: "tool",
    text: title,
    toolKind: toolKindFromName(name),
    status: "in_progress",
    ...(preview ? { preview } : {}),
  });
}

/**
 * What a subagent said and thought on its way through the work. Its prose
 * never joins the parent transcript — that would read as the main agent
 * talking — but it is the most legible thing in the panel for its own row.
 */
function noteSubagentNarration(
  live: Live,
  rec: Record<string, unknown>,
): void {
  const parent = subagentParent(live, rec);
  if (!parent) return;
  const model = stringField(asRecord(rec.message), "model");
  if (model)
    live.onEvent({
      type: "tool.updated",
      callId: parent.id,
      kind: "agent",
      agentModel: model,
    });
  const messageId = assistantMessageId(rec) ?? crypto.randomUUID();
  const thinking = assistantThinkingBlocks(rec).join("").trim();
  if (thinking) {
    live.onEvent({
      type: "agent.step",
      callId: parent.id,
      stepId: `${messageId}:thinking`,
      kind: "reasoning",
      text: thinking,
    });
  }
  const text = assistantTextBlocks(rec).join("").trim();
  if (text) {
    live.onEvent({
      type: "agent.step",
      callId: parent.id,
      stepId: `${messageId}:text`,
      kind: "message",
      text,
    });
  }
}

/** Settles the subagent's own tool rows once their results come back. */
function noteSubagentResults(
  live: Live,
  rec: Record<string, unknown>,
): void {
  const parent = subagentParent(live, rec);
  if (!parent) return;
  for (const result of toolResultsFromUserMessage(rec)) {
    live.onEvent({
      type: "agent.step",
      callId: parent.id,
      stepId: result.toolUseId,
      kind: "tool",
      text: "",
      status: result.isError ? "failed" : "completed",
    });
  }
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
): void {
  const id = callId ?? `agent:${title}`;
  const existing = live.toolsById.get(id);
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
    return;
  }
  if (title) existing.title = title;
  live.onEvent({
    type: "tool.updated",
    callId: id,
    title: existing.title,
    kind: "agent",
    status,
    ...(detail ? { detail } : {}),
  });
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
    upsertAgentTool(
      live,
      task.toolUseId,
      task.description,
      status,
      detail ?? (status === "failed" ? "Subagent failed." : undefined),
    );
  }
  maybeFinishTurn(live);
}

/**
 * A task is done. If Claude had already yielded, the notification about it
 * starts a follow-up turn, so hold the MonoCode turn open for that too rather
 * than settling in the gap between the two.
 */
function finishBackgroundTask(live: Live, taskId: string): void {
  if (!live.backgroundTasks.delete(taskId)) return;
  if (live.turnResultSeen && live.activeTurn && !live.awaitingResume) {
    live.awaitingResume = setTimeout(() => {
      live.awaitingResume = null;
      maybeFinishTurn(live);
      syncBackgroundWait(live);
    }, RESUME_GRACE_MS);
  }
  maybeFinishTurn(live);
  syncBackgroundWait(live);
}

/**
 * Claude began another turn inside this MonoCode turn: woken by a finished
 * task, or by a follow-up written in while it waited. Its own result, not the
 * earlier one, decides when the MonoCode turn ends.
 */
function noteClaudeTurnStarted(live: Live): void {
  if (!live.activeTurn || !live.turnResultSeen) return;
  live.turnResultSeen = false;
  // A new message, not more of the last one: its snapshot must not be
  // compared against what the earlier turn streamed.
  live.emittedAssistant = "";
  live.emittedReasoning = "";
  live.pendingAssistantBoundary = false;
  // Close the message Claude left off with, so the reply starts its own and
  // the fold puts the earlier one away, the same as prose between tool calls.
  // A background command's row already sits between the two; a subagent's
  // report is noted in the trail to do the same.
  live.onEvent({ type: "message.completed" });
  live.onEvent({ type: "reasoning.completed" });
  for (const text of live.taskNotes) live.onEvent({ type: "status", text });
  live.taskNotes = [];
  clearAwaitingResume(live);
  syncBackgroundWait(live);
}

/**
 * Claude yielded with commands still running. Each gets a live row under the
 * message it left off with, like any call in flight, until it finishes.
 * Subagents already have a row of their own.
 */
function showBackgroundRows(live: Live): void {
  if (!live.activeTurn || live.cancelled) return;
  for (const [taskId, task] of live.backgroundTasks) {
    if (live.backgroundRows.has(taskId) || live.agentTasks.has(taskId)) continue;
    const source = task.toolUseId
      ? live.toolsById.get(task.toolUseId)
      : undefined;
    if (source && isAgentToolName(source.name)) continue;
    const callId = `background:${taskId}`;
    live.backgroundRows.set(taskId, callId);
    live.onEvent({
      type: "tool.started",
      callId,
      title: source ? toolTitle(source.name, source.input) : task.description,
      kind: source ? toolKindFromName(source.name) : "execute",
      status: "in_progress",
      background: true,
      ...(source ? { preview: previewFromTool(source.name, source.input) } : {}),
    });
  }
}

function settleBackgroundRow(
  live: Live,
  taskId: string,
  status: string,
  detail?: string,
): void {
  const callId = live.backgroundRows.get(taskId);
  if (!callId || live.muteUpdates) return;
  live.onEvent({
    type: "tool.updated",
    callId,
    status: status === "completed" ? "completed" : "failed",
    ...(detail ? { detail } : {}),
  });
}

/**
 * What Claude was told when a task finished. A command's row takes the
 * summary; a subagent's is kept for the trail until Claude picks back up.
 */
function noteTaskNotification(
  live: Live,
  notice: ClaudeAgentTaskNotification,
): void {
  if (!live.activeTurn) return;
  if (live.backgroundRows.has(notice.taskId)) {
    settleBackgroundRow(
      live,
      notice.taskId,
      notice.status,
      notice.summary || undefined,
    );
    return;
  }
  const tool = notice.toolUseId ? live.toolsById.get(notice.toolUseId) : null;
  const agent =
    (tool && isAgentToolName(tool.name)) || live.agentTasks.has(notice.taskId);
  if (!agent || !live.turnResultSeen) return;
  live.taskNotes.push(notice.summary || "Subagent finished.");
}

function clearAwaitingResume(live: Live): void {
  if (!live.awaitingResume) return;
  clearTimeout(live.awaitingResume);
  live.awaitingResume = null;
}

/**
 * Tells the UI what the turn is waiting on once Claude has yielded with work
 * still running, and clears it when Claude picks the thread back up.
 */
function syncBackgroundWait(live: Live): void {
  const waiting =
    live.activeTurn && live.turnResultSeen && !live.cancelled
      ? [...live.backgroundTasks.values()].map((task) => task.description)
      : [];
  const key = waiting.join("\n");
  if (key === live.backgroundKey) return;
  live.backgroundKey = key;
  if (live.muteUpdates) return;
  live.onEvent({ type: "background.updated", tasks: waiting });
}

function maybeFinishTurn(live: Live): void {
  if (!live.turnResultSeen) return;
  if (live.agentTasks.size > 0 || live.backgroundTasks.size > 0) return;
  if (live.awaitingResume) return;
  if (!live.activeTurn && !live.turnDone) return;
  finishActiveTurn(live, [
    { type: "message.completed" },
    { type: "reasoning.completed" },
  ]);
}

function finishActiveTurn(live: Live, extraEvents: HarnessEvent[] = []): void {
  clearAwaitingResume(live);
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
  return `${input.providerAccountId ?? "default"}:${claudeSettingsKey({
    model: nativeModelId(input.model),
    effort: input.modelSettings?.effort,
    fast: input.modelSettings?.fast,
    thinking: input.modelSettings?.thinking,
    context: input.modelSettings?.context,
    runtimeMode: input.runtimeMode,
    hooks: loadClaudeHooks(),
  })}`;
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
