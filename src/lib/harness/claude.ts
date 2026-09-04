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
import { isAgentToolName } from "./preview";
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
  agentTasks: Map<string, LiveAgentTask>;
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
    agentTasks: new Map(),
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
  live.agentTasks.clear();
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

function handleStreamEvent(live: Live, rec: Record<string, unknown>): void {
  const subagent = isSubagentMessage(rec);
  const delta = streamDeltaFromEvent(rec);
  if (delta) {
    if (subagent) return;
    if (delta.kind === "assistant") {
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
      noteSubagentTool(live, rec, started.name, started.input);
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
    for (const use of assistantToolUses(rec)) {
      noteSubagentTool(live, rec, use.name, use.input);
    }
    return;
  }

  const used = contextUsedFromAssistant(rec);
  if (used !== undefined) live.onEvent({ type: "context", used });

  const snapshot = assistantTextBlocks(rec).join("");
  const extra = snapshotRemainder(live.emittedAssistant, snapshot);
  if (extra) {
    live.emittedAssistant = joinStreamText(live.emittedAssistant, extra);
    live.onEvent({ type: "message.delta", text: extra });
  }

  for (const use of assistantToolUses(rec)) {
    if (live.toolsById.has(use.id)) continue;
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
      status: isAgentToolName(tool.name) ? "in_progress" : "pending",
      preview: previewFromTool(tool.name, tool.input),
    });
    if (use.name === "ExitPlanMode") {
      const plan = extractExitPlanModePlan(use.input);
      if (plan) live.onEvent({ type: "plan", text: plan });
    }
    emitTaskListIfNeeded(live, tool.name, tool.input);
  }
}

function handleUser(live: Live, rec: Record<string, unknown>): void {
  if (isSubagentMessage(rec)) return;
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
}

function noteSubagentTool(
  live: Live,
  rec: Record<string, unknown>,
  name: string,
  input: Record<string, unknown>,
): void {
  const parentId = stringField(rec, "parent_tool_use_id");
  if (!parentId) return;
  const parent = live.toolsById.get(parentId);
  if (!parent || !isAgentToolName(parent.name)) return;
  live.onEvent({
    type: "tool.updated",
    callId: parent.id,
    title: parent.title,
    kind: "agent",
    status: "in_progress",
    detail: toolTitle(name, input),
  });
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
    upsertAgentTool(live, task.toolUseId, task.description, status, detail);
  }
  maybeFinishTurn(live);
}

function maybeFinishTurn(live: Live): void {
  if (!live.turnResultSeen) return;
  if (live.agentTasks.size > 0) return;
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
