import { nativeModelId } from "../models";
import type { RuntimeMode } from "../session";
import { promptBlocks } from "../attachments";
import { isTaskListToolName, taskListFromToolInput } from "../taskList";
import { AcpClient, type AcpHandlers } from "./acp";
import {
  killChild,
  resolveCursorBinary,
  spawnChild,
  unwatchChild,
  watchChild,
} from "./child";
import {
  readStoredCursorToolCalls,
  type StoredCursorToolCall,
} from "./cursorStore";
import { stopCursorTitleGeneration } from "./cursorTitle";
import type {
  ApprovalDecision,
  HarnessEvent,
  SendTurnInput,
  SteerTurnInput,
} from "./types";
import {
  CUSTOM_OPTION_ID,
  questionPromptTitle,
  questionsFromUnknown,
  type UserQuestion,
  type UserQuestionReply,
} from "../userQuestion";
import {
  agentToolTitle,
  composeToolTitle,
  extractSearchQuery,
  extractShellCommand,
  extractSkillName,
  extractToolPreview,
  isAgentTool,
  isAgentToolName,
  isWeakToolTitle,
  mergeToolPreview,
} from "./preview";

type SessionConfigOption = {
  id: string;
  category?: string;
  currentValue?: string | boolean;
};

type SessionSetupResult = {
  sessionId?: string;
  configOptions?: unknown;
};

type PendingToolEnrichment = {
  kind?: string;
  attempts: number;
};

type Live = {
  acp: AcpClient;
  acpSessionId: string;
  cwd: string;
  modelConfigId: string;
  configOptions: SessionConfigOption[];
  muteUpdates: boolean;
  cancelled: boolean;
  runtimeMode: RuntimeMode;
  planning: boolean;
  onEvent: (event: HarnessEvent) => void;
  approvals: Map<number, (decision: ApprovalDecision) => void>;
  questions: Map<number, (reply: UserQuestionReply) => void>;
  enrichedTools: Set<string>;
  pendingToolEnrichments: Map<string, PendingToolEnrichment>;
  toolEnrichmentTimer?: ReturnType<typeof setTimeout>;
  toolEnrichmentRunning: boolean;
  toolStatuses: Map<string, string>;
  taskListTools: Set<string>;
  agentTools: Map<string, string>;
  backgroundAgentTools: Set<string>;
  promptActive: boolean;
  turns: Promise<void>;
};

type Resume = {
  acpSessionId: string;
  cwd: string;
};

const liveByThread = new Map<string, Live>();
const resumeByThread = new Map<string, Resume>();
const cancelledThreads = new Set<string>();

const CLIENT_CAPABILITIES = {
  fs: { readTextFile: false, writeTextFile: false },
  terminal: false,
  _meta: { parameterizedModelPicker: true },
};

export async function sendCursorTurn(input: SendTurnInput): Promise<void> {
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
      scheduleCursorToolEnrichment(live, 0);
      try {
        await applyModelSelection(live, input);
        if (live.cancelled) return;
        await prompt(live, input);
      } catch (error) {
        if (live.cancelled) return;
        throw error;
      }
    });
  await live.turns;
}

export async function steerCursorTurn(input: SteerTurnInput): Promise<void> {
  const live = liveByThread.get(input.sessionId);
  if (!live) throw new Error("No active Cursor session");

  const blocks = promptBlocks(input.text, input.attachments);
  if (blocks.length === 0) return;

  const params = {
    sessionId: live.acpSessionId,
    prompt: blocks,
  };
  try {
    await live.acp.notify("session/steer", params);
  } catch {
    await live.acp.notify("_session/steer", params);
  }
}

export function respondCursorApproval(
  sessionId: string,
  requestId: number,
  decision: ApprovalDecision,
) {
  liveByThread.get(sessionId)?.approvals.get(requestId)?.(decision);
}

export function respondCursorQuestion(
  sessionId: string,
  requestId: number,
  reply: UserQuestionReply,
) {
  liveByThread.get(sessionId)?.questions.get(requestId)?.(reply);
}

/** Abort the in-flight prompt without tearing down the ACP session. */
export async function cancelCursorTurn(sessionId: string): Promise<void> {
  const live = liveByThread.get(sessionId);
  if (!live) {
    cancelledThreads.add(sessionId);
    return;
  }
  live.cancelled = true;
  live.muteUpdates = true;
  live.promptActive = false;
  live.taskListTools.clear();
  live.agentTools.clear();
  live.backgroundAgentTools.clear();
  if (live.toolEnrichmentTimer) clearTimeout(live.toolEnrichmentTimer);
  live.toolEnrichmentTimer = undefined;
  for (const [, resolve] of live.approvals) resolve("deny");
  live.approvals.clear();
  for (const [, resolve] of live.questions) resolve({ kind: "skipped" });
  live.questions.clear();
  await live.acp
    .notify("session/cancel", { sessionId: live.acpSessionId })
    .catch(() => undefined);
  live.acp.rejectPending(new Error("cancelled"));
}

/** Kill the Cursor process but keep the ACP session id so we can session/load. */
export async function stopCursorSession(sessionId: string): Promise<void> {
  cancelledThreads.delete(sessionId);
  const live = liveByThread.get(sessionId);
  liveByThread.delete(sessionId);
  if (live) {
    live.muteUpdates = true;
    live.promptActive = false;
    if (live.toolEnrichmentTimer) clearTimeout(live.toolEnrichmentTimer);
    live.pendingToolEnrichments.clear();
    live.agentTools.clear();
    live.backgroundAgentTools.clear();
    for (const [, resolve] of live.approvals) resolve("deny");
    live.approvals.clear();
    for (const [, resolve] of live.questions) resolve({ kind: "skipped" });
    live.questions.clear();
  }
  live?.acp.close();
  unwatchChild(sessionId);
  await killChild(sessionId).catch(() => undefined);
}

/** Delete or idle detach — drop the Cursor conversation too. */
export async function forgetCursorSession(sessionId: string): Promise<void> {
  resumeByThread.delete(sessionId);
  await stopCursorSession(sessionId);
  await stopCursorTitleGeneration(sessionId);
}

/** Seed ACP resume state for a restored MonoCode session. */
export function bindCursorSession(
  threadId: string,
  acpSessionId: string,
  cwd: string,
): void {
  const sessionId = acpSessionId.trim();
  if (!threadId || !sessionId || !cwd.trim()) return;
  resumeByThread.set(threadId, { acpSessionId: sessionId, cwd });
}

async function ensureLive(input: SendTurnInput): Promise<Live> {
  const existing = liveByThread.get(input.sessionId);
  if (existing && existing.cwd === input.cwd) {
    existing.onEvent = input.onEvent;
    existing.runtimeMode = input.runtimeMode;
    existing.planning = input.intent === "plan";
    return existing;
  }
  if (existing) {
    resumeByThread.delete(input.sessionId);
    await stopCursorSession(input.sessionId);
  }

  const resume = resumeByThread.get(input.sessionId);
  const canLoad = resume != null && resume.cwd === input.cwd;
  if (resume && resume.cwd !== input.cwd) {
    resumeByThread.delete(input.sessionId);
  }

  const { path } = await resolveCursorBinary();
  const handlers: AcpHandlers = {};
  const acp = new AcpClient(input.sessionId, handlers);
  const liveRef: { current: Live | null } = { current: null };
  const muteGate = { current: false };

  handlers.onNotification = (method, params) => {
    if (muteGate.current) return;
    const live = liveRef.current;
    if (!live || live.muteUpdates) return;
    handleNotification(live, method, params);
  };
  handlers.onRequest = (id, method, params) => {
    const live = liveRef.current;
    if (!live) return;
    void handleRequest(live, id, method, params);
  };

  watchChild(
    input.sessionId,
    (line) => acp.pushLine(line),
    (code) => {
      acp.close(new Error("Cursor CLI exited"));
      liveByThread.delete(input.sessionId);
      input.onEvent({ type: "session.ended", code });
    },
  );

  await spawnChild(input.sessionId, path, ["acp"], input.cwd);

  try {
    await acp.request("initialize", {
      protocolVersion: 1,
      clientCapabilities: CLIENT_CAPABILITIES,
      clientInfo: { name: "monocode", version: "0.1.0" },
    });
    await acp
      .request("authenticate", { methodId: "cursor_login" })
      .catch(() => undefined);

    let setup: SessionSetupResult | undefined;
    let acpSessionId: string | undefined;
    let didLoad = false;

    if (canLoad && resume) {
      muteGate.current = true;
      try {
        setup = await acp.request<SessionSetupResult>("session/load", {
          sessionId: resume.acpSessionId,
          cwd: input.cwd,
          mcpServers: [],
        });
        acpSessionId = resume.acpSessionId;
        didLoad = true;
      } catch {
        setup = undefined;
        acpSessionId = undefined;
        didLoad = false;
      } finally {
        muteGate.current = false;
      }
    }

    if (!acpSessionId) {
      setup = await acp.request<SessionSetupResult>("session/new", {
        cwd: input.cwd,
        mcpServers: [],
      });
      acpSessionId = setup.sessionId?.trim();
    }
    if (!acpSessionId) throw new Error("Cursor did not return a session id");

    const live: Live = {
      acp,
      acpSessionId,
      cwd: input.cwd,
      modelConfigId: extractModelConfigId(setup),
      configOptions: readConfigOptions(setup?.configOptions),
      muteUpdates: didLoad,
      cancelled: false,
      runtimeMode: input.runtimeMode,
      planning: input.intent === "plan",
      onEvent: input.onEvent,
      approvals: new Map(),
      questions: new Map(),
      enrichedTools: new Set(),
      pendingToolEnrichments: new Map(),
      toolEnrichmentRunning: false,
      toolStatuses: new Map(),
      taskListTools: new Set(),
      agentTools: new Map(),
      backgroundAgentTools: new Set(),
      promptActive: false,
      turns: Promise.resolve(),
    };
    liveRef.current = live;
    liveByThread.set(input.sessionId, live);
    resumeByThread.set(input.sessionId, {
      acpSessionId,
      cwd: input.cwd,
    });
    live.onEvent({
      type: "session.providerBound",
      providerSessionId: acpSessionId,
    });
    live.onEvent({ type: "session.started" });
    return live;
  } catch (error) {
    acp.close(error instanceof Error ? error : new Error(String(error)));
    await stopCursorSession(input.sessionId);
    throw error;
  }
}

async function applyModelSelection(
  live: Live,
  input: SendTurnInput,
): Promise<void> {
  const base = nativeModelId(input.model);
  const settings = input.modelSettings ?? {};

  try {
    await setConfigOption(live, live.modelConfigId, base);
  } catch {
    await live.acp
      .request("session/set_model", {
        sessionId: live.acpSessionId,
        modelId: base,
      })
      .catch(() => undefined);
  }

  for (const [settingId, value] of Object.entries(settings)) {
    const configId = resolveSettingConfigId(live.configOptions, settingId);
    if (!configId) continue;
    await setConfigOption(live, configId, value).catch(() => undefined);
  }
}

async function setConfigOption(
  live: Live,
  configId: string,
  value: string | boolean,
): Promise<void> {
  const current = live.configOptions.find((option) => option.id === configId);
  if (current && String(current.currentValue ?? "") === String(value)) return;

  const result = await live.acp.request<SessionSetupResult>(
    "session/set_config_option",
    {
      sessionId: live.acpSessionId,
      configId,
      value,
    },
  );
  if (result?.configOptions) {
    live.configOptions = readConfigOptions(result.configOptions);
    live.modelConfigId = extractModelConfigId(result) || live.modelConfigId;
  }
}

async function prompt(live: Live, input: SendTurnInput): Promise<void> {
  try {
    const blocks = promptBlocks(input.text, input.attachments);
    if (blocks.length === 0) return;
    live.agentTools.clear();
    live.backgroundAgentTools.clear();
    live.taskListTools.clear();
    live.promptActive = true;
    await live.acp.request("session/prompt", {
      sessionId: live.acpSessionId,
      prompt: blocks,
    });
    live.promptActive = false;
    if (live.cancelled) {
      live.backgroundAgentTools.clear();
      return;
    }
    settleCursorBackgroundAgents(live, "completed");
    live.onEvent({ type: "message.completed" });
    live.onEvent({ type: "reasoning.completed" });
    wakeCursorToolEnrichment(live);
  } catch (error) {
    live.promptActive = false;
    if (live.cancelled) return;
    settleCursorBackgroundAgents(live, "failed");
    live.onEvent({
      type: "session.error",
      message: error instanceof Error ? error.message : String(error),
    });
    throw error;
  }
}

function handleNotification(live: Live, method: string, params: unknown) {
  if (method === "session/update") {
    handleSessionUpdate(live, params);
    return;
  }
  if (isCursorTodoUpdate(method)) {
    emitCursorTodoUpdate(live, params);
  }
}

async function handleRequest(
  live: Live,
  id: number,
  method: string,
  params: unknown,
) {
  if (method === "session/request_permission") {
    await handlePermission(live, id, params);
    return;
  }
  if (method === "cursor/ask_question") {
    await handleAskQuestion(live, id, params);
    return;
  }
  if (method === "cursor/create_plan") {
    const rec = asRecord(params);
    const plan = typeof rec?.plan === "string" ? rec.plan : "";
    if (plan) live.onEvent({ type: "plan", text: plan });
    await live.acp.respond(id, { outcome: { outcome: "accepted" } });
    return;
  }
  if (isCursorTodoUpdate(method)) {
    emitCursorTodoUpdate(live, params);
    await live.acp.respond(id, {}).catch(() => undefined);
    return;
  }
  if (method === "cursor/task") {
    handleCursorTask(live, params);
    await live.acp.respond(id, {}).catch(() => undefined);
    return;
  }
  await live.acp.respond(id, {}).catch(() => undefined);
}

function isCursorTodoUpdate(method: string): boolean {
  return method === "cursor/update_todos" || method === "_cursor/update_todos";
}

function emitCursorTodoUpdate(live: Live, params: unknown): void {
  const rec = asRecord(params);
  const callId = rec
    ? (stringField(rec, "toolCallId") ?? stringField(rec, "tool_call_id"))
    : undefined;
  if (callId) {
    live.taskListTools.add(callId);
    live.onEvent({ type: "tool.updated", callId, kind: "tasks" });
  }
  const items = taskListFromToolInput("updateTodos", params);
  if (!items) return;
  live.onEvent({
    type: "tasks.updated",
    items,
    ...(rec?.merge === true ? { merge: true } : {}),
  });
}

function handleCursorTask(live: Live, params: unknown): void {
  const task = asRecord(params);
  if (!task || !live.promptActive) return;
  const callId =
    stringField(task, "toolCallId") ?? stringField(task, "tool_call_id");
  if (!callId || !live.agentTools.has(callId)) return;

  const agentId = stringField(task, "agentId") ?? stringField(task, "agent_id");
  const durationMs =
    numberField(task, "durationMs") ?? numberField(task, "duration_ms");
  const background =
    live.backgroundAgentTools.has(callId) ||
    (!!agentId && durationMs === undefined);
  if (!background) return;

  const title =
    stringField(task, "description") ??
    live.agentTools.get(callId) ??
    "Subagent";
  live.agentTools.set(callId, title);
  live.backgroundAgentTools.add(callId);
  live.toolStatuses.set(callId, "in_progress");
  live.onEvent({
    type: "tool.updated",
    callId,
    title,
    kind: "agent",
    status: "in_progress",
    detail: cursorSubagentDetail(task),
  });
}

async function handleAskQuestion(live: Live, id: number, params: unknown) {
  const rec = asRecord(params);
  const questions = questionsFromUnknown(params);
  const title =
    (typeof rec?.title === "string" && rec.title.trim()) ||
    questionPromptTitle(questions);
  const callId =
    typeof rec?.toolCallId === "string"
      ? rec.toolCallId
      : typeof rec?.tool_call_id === "string"
        ? rec.tool_call_id
        : undefined;
  live.onEvent({
    type: "question.asked",
    requestId: id,
    title,
    questions,
    ...(callId ? { callId } : {}),
  });

  const reply = await new Promise<UserQuestionReply>((resolve) => {
    live.questions.set(id, resolve);
  });
  live.questions.delete(id);
  live.onEvent({
    type: "question.resolved",
    requestId: id,
    decision: reply.kind,
  });

  await live.acp
    .respond(id, cursorAskQuestionResponse(reply, questions))
    .catch(() => undefined);
}

function cursorAskQuestionResponse(
  reply: UserQuestionReply,
  questions: UserQuestion[],
): Record<string, unknown> {
  if (reply.kind !== "answered") {
    return { outcome: { outcome: "skipped", reason: "User skipped" } };
  }
  return {
    outcome: {
      outcome: "answered",
      answers: questions.map((question) => ({
        questionId: question.id,
        selectedOptionIds: (reply.answers[question.id] ?? []).filter(
          (optionId) => optionId !== CUSTOM_OPTION_ID,
        ),
      })),
    },
  };
}

async function handlePermission(live: Live, id: number, params: unknown) {
  const rec = asRecord(params);
  const subject = asRecord(rec?.subject);
  const tool =
    asRecord(rec?.toolCall) ??
    asRecord(subject?.toolCall) ??
    asRecord(subject) ??
    rec ??
    {};
  const command = stringField(subject ?? {}, "command");
  const kind = stringField(tool, "kind") ?? stringField(subject ?? {}, "kind");
  const preview = mergeToolPreview(
    extractToolPreview(tool, tool),
    subject ? extractToolPreview(subject, subject) : undefined,
  );
  const title =
    composeToolTitle({
      kind,
      title:
        toolLabel(tool, subject ?? tool) ??
        command ??
        stringField(rec ?? {}, "title"),
      command:
        command ??
        extractShellCommand(tool.rawInput, tool.raw_input, tool.input, subject),
      skill: extractSkillName(
        tool.rawInput,
        tool.raw_input,
        tool.input,
        subject,
      ),
      path: preview?.path,
      query:
        preview?.query ??
        extractSearchQuery(tool) ??
        extractSearchQuery(subject),
      previewKind: preview?.kind,
    }) || "Permission";
  const callId =
    stringField(tool, "toolCallId") ??
    stringField(tool, "tool_call_id") ??
    stringField(rec ?? {}, "toolCallId") ??
    stringField(subject ?? {}, "toolCallId");
  if (callId) {
    live.onEvent({
      type: "tool.updated",
      callId,
      title,
      kind,
      status: live.toolStatuses.get(callId),
      preview,
    });
    if (preview?.path || preview?.query) {
      live.enrichedTools.add(callId);
      live.pendingToolEnrichments.delete(callId);
    } else if (needsCursorToolEnrichment(kind, title, preview)) {
      queueCursorToolEnrichment(live, callId, kind);
    }
  }

  const options = Array.isArray(rec?.options) ? rec.options : [];
  const optionIds = options
    .map((item) => asRecord(item)?.optionId)
    .filter((value): value is string => typeof value === "string");

  if (live.planning) {
    const normalized = (preview?.kind ?? kind ?? "").toLowerCase();
    const readOnly = normalized === "read" || normalized === "search";
    const optionId = readOnly
      ? pickOption(optionIds, ["allow-once", "allow_once", "allow"])
      : pickOption(optionIds, ["reject-once", "reject_once", "reject-always"]);
    await live.acp.respond(id, {
      outcome: {
        outcome: "selected",
        optionId: optionId ?? (readOnly ? "allow-once" : "reject-once"),
      },
    });
    return;
  }

  const auto = pickAutoOption(live.runtimeMode, kind, optionIds);
  if (auto) {
    await live.acp.respond(id, {
      outcome: { outcome: "selected", optionId: auto },
    });
    return;
  }

  live.onEvent({
    type: "approval.requested",
    requestId: id,
    title,
    kind,
    callId,
    preview,
  });

  const decision = await new Promise<ApprovalDecision>((resolve) => {
    live.approvals.set(id, resolve);
  });
  live.approvals.delete(id);
  live.onEvent({ type: "approval.resolved", requestId: id, decision });

  const optionId =
    decision === "allow"
      ? pickOption(optionIds, [
          "allow-once",
          "allow_once",
          "allow-always",
          "allow_always",
        ])
      : pickOption(optionIds, ["reject-once", "reject_once", "reject-always"]);

  await live.acp.respond(id, {
    outcome: {
      outcome: "selected",
      optionId:
        optionId ?? (decision === "allow" ? "allow-once" : "reject-once"),
    },
  });
}

function handleSessionUpdate(live: Live, params: unknown) {
  const rec = asRecord(params);
  const update = asRecord(rec?.update) ?? rec;
  if (!update) return;
  const kind = String(
    update.sessionUpdate ?? update.session_update ?? update.type ?? "",
  );

  if (kind === "agent_message_chunk" || kind === "agent_message") {
    // Whole-message arrays contain distinct content blocks; chunks are exact deltas.
    const text = textFromContent(
      update.content ?? update.text,
      kind === "agent_message" ? "\n" : "",
    );
    if (text) live.onEvent({ type: "message.delta", text });
    return;
  }
  if (kind === "agent_thought_chunk" || kind === "agent_thought") {
    const text = textFromContent(
      update.content ?? update.text,
      kind === "agent_thought" ? "\n" : "",
    );
    if (text) live.onEvent({ type: "reasoning.delta", text });
    return;
  }
  if (
    kind === "tool_call" ||
    kind === "tool_call_update" ||
    kind === "tool_call_content_chunk"
  ) {
    const tool =
      asRecord(update.toolCall) ?? asRecord(update.tool_call) ?? update;
    const callId = String(
      tool.toolCallId ??
        tool.tool_call_id ??
        update.toolCallId ??
        update.tool_call_id ??
        "",
    );
    if (!callId) return;
    const reportedKind =
      coerceMaybeString(update, "kind") ?? coerceMaybeString(tool, "kind");
    const status =
      coerceMaybeString(update, "status") ?? coerceMaybeString(tool, "status");
    const rawTitle = toolLabel(update, tool);
    const rawInput =
      update.rawInput ??
      tool.rawInput ??
      update.raw_input ??
      tool.raw_input ??
      update.input ??
      tool.input;
    const agent =
      live.agentTools.has(callId) ||
      isAgentTool(reportedKind, rawTitle) ||
      isCursorAgentInput(rawInput);
    const taskList =
      live.taskListTools.has(callId) ||
      isCursorTaskListInput(rawInput, rawTitle);
    if (taskList) live.taskListTools.add(callId);
    const toolKind = agent ? "agent" : taskList ? "tasks" : reportedKind;
    const detail = toolDetail(update, tool);
    const preview = extractToolPreview(update, tool);
    const title = agent
      ? cursorAgentTitle(rawInput, rawTitle, live.agentTools.get(callId))
      : composeToolTitle({
          kind: toolKind,
          title: rawTitle,
          command: extractShellCommand(
            update.rawInput,
            tool.rawInput,
            update.raw_input,
            tool.raw_input,
            update.input,
            tool.input,
          ),
          skill: extractSkillName(
            update.rawInput,
            tool.rawInput,
            update.raw_input,
            tool.raw_input,
            update.input,
            tool.input,
          ),
          path: preview?.path,
          query: preview?.query ?? extractSearchQuery(rawInput),
          previewKind: preview?.kind,
        }) || rawTitle;
    if (agent && title) live.agentTools.set(callId, title);
    const background =
      agent &&
      status === "completed" &&
      cursorToolOutputIsBackground(update, tool);
    if (background) live.backgroundAgentTools.add(callId);
    const displayedStatus = background ? "in_progress" : status;
    if (displayedStatus) live.toolStatuses.set(callId, displayedStatus);
    live.onEvent({
      type: "tool.updated",
      callId,
      title,
      kind: toolKind,
      status: displayedStatus,
      detail,
      preview,
    });
    if (needsCursorToolEnrichment(toolKind, title, preview)) {
      queueCursorToolEnrichment(live, callId, toolKind);
    } else if (live.pendingToolEnrichments.has(callId)) {
      live.pendingToolEnrichments.delete(callId);
      live.enrichedTools.add(callId);
    }
  }
}

function isCursorAgentInput(value: unknown): boolean {
  const input = asRecord(value);
  if (!input) return false;
  const name =
    stringField(input, "_toolName") ??
    stringField(input, "toolName") ??
    stringField(input, "tool_name") ??
    stringField(input, "name");
  return !!name && isAgentToolName(name);
}

function isCursorTaskListInput(
  value: unknown,
  title: string | undefined,
): boolean {
  const input = asRecord(value);
  const name = input
    ? (stringField(input, "_toolName") ??
      stringField(input, "toolName") ??
      stringField(input, "tool_name") ??
      stringField(input, "name"))
    : undefined;
  const normalized = name?.replace(/[\s_-]+/g, "").toLowerCase();
  return (
    (!!normalized && isTaskListToolName(normalized)) ||
    /^update todos\b/i.test(title ?? "")
  );
}

function cursorAgentTitle(
  rawInput: unknown,
  rawTitle: string | undefined,
  existing: string | undefined,
): string {
  const input = asRecord(rawInput);
  if (input) return agentToolTitle(input, existing ?? rawTitle ?? "Subagent");
  if (existing) return existing;
  const stripped = rawTitle
    ?.replace(/^(?:agent|task|subagent)\b[\s:·-]*/i, "")
    .trim();
  return stripped || "Subagent";
}

function cursorToolOutputIsBackground(
  update: Record<string, unknown>,
  tool: Record<string, unknown>,
): boolean {
  for (const value of [
    update.rawOutput,
    tool.rawOutput,
    update.raw_output,
    tool.raw_output,
  ]) {
    const output = asRecord(value);
    if (output?.isBackground === true || output?.is_background === true) {
      return true;
    }
  }
  return false;
}

function cursorSubagentDetail(
  task: Record<string, unknown>,
): string | undefined {
  const type = task.subagentType ?? task.subagent_type;
  if (typeof type === "string" && type && type !== "unspecified") {
    return `${type.replace(/[_-]+/g, " ")} subagent`;
  }
  const custom = asRecord(type)?.custom;
  if (typeof custom === "string" && custom.trim()) {
    return `${custom.trim().replace(/[_-]+/g, " ")} subagent`;
  }
  return undefined;
}

function settleCursorBackgroundAgents(
  live: Live,
  status: "completed" | "failed",
): void {
  for (const callId of live.backgroundAgentTools) {
    live.toolStatuses.set(callId, status);
    live.onEvent({
      type: "tool.updated",
      callId,
      title: live.agentTools.get(callId),
      kind: "agent",
      status,
    });
  }
  live.backgroundAgentTools.clear();
}

const TOOL_ENRICH_MAX_ATTEMPTS = 20;

function needsCursorToolEnrichment(
  kind: string | undefined,
  title: string | undefined,
  preview: ReturnType<typeof extractToolPreview>,
): boolean {
  const key = (kind ?? "").toLowerCase();
  if (
    key === "execute" ||
    key === "think" ||
    key === "fetch" ||
    key === "skill"
  )
    return false;
  if (preview?.path || preview?.query) return false;
  if (key === "read" || key === "search" || key === "edit" || key === "write") {
    return true;
  }
  return !title || isWeakToolTitle(title);
}

function queueCursorToolEnrichment(
  live: Live,
  callId: string,
  kind?: string,
): void {
  if (live.enrichedTools.has(callId)) return;
  const pending = live.pendingToolEnrichments.get(callId);
  live.pendingToolEnrichments.set(callId, {
    kind: kind ?? pending?.kind,
    attempts: pending?.attempts ?? 0,
  });
  scheduleCursorToolEnrichment(live, 0);
}

function scheduleCursorToolEnrichment(live: Live, delay: number): void {
  if (
    live.muteUpdates ||
    live.toolEnrichmentRunning ||
    live.toolEnrichmentTimer ||
    live.pendingToolEnrichments.size === 0
  ) {
    return;
  }
  live.toolEnrichmentTimer = setTimeout(() => {
    live.toolEnrichmentTimer = undefined;
    void refreshCursorToolEnrichments(live);
  }, delay);
}

function wakeCursorToolEnrichment(live: Live): void {
  if (live.toolEnrichmentTimer) clearTimeout(live.toolEnrichmentTimer);
  live.toolEnrichmentTimer = undefined;
  scheduleCursorToolEnrichment(live, 0);
}

async function refreshCursorToolEnrichments(live: Live): Promise<void> {
  if (
    live.muteUpdates ||
    live.toolEnrichmentRunning ||
    live.pendingToolEnrichments.size === 0
  ) {
    return;
  }
  live.toolEnrichmentRunning = true;
  const callIds = [...live.pendingToolEnrichments.keys()].slice(0, 256);
  try {
    const storedCalls = await readStoredCursorToolCalls(
      live.acpSessionId,
      callIds,
    ).catch(() => []);
    if (live.muteUpdates) return;

    for (const stored of storedCalls) {
      const pending = live.pendingToolEnrichments.get(stored.toolCallId);
      if (!pending) continue;
      if (applyStoredCursorToolCall(live, stored, pending.kind)) {
        live.pendingToolEnrichments.delete(stored.toolCallId);
      }
    }

    for (const callId of callIds) {
      const pending = live.pendingToolEnrichments.get(callId);
      if (!pending) continue;
      const attempts = pending.attempts + 1;
      if (attempts >= TOOL_ENRICH_MAX_ATTEMPTS) {
        live.pendingToolEnrichments.delete(callId);
      } else {
        live.pendingToolEnrichments.set(callId, { ...pending, attempts });
      }
    }
  } finally {
    live.toolEnrichmentRunning = false;
    scheduleCursorToolEnrichment(live, toolEnrichmentDelay(live));
  }
}

function applyStoredCursorToolCall(
  live: Live,
  stored: StoredCursorToolCall,
  kind?: string,
): boolean {
  const mappedKind = kindFromCursorToolName(stored.toolName, kind);
  const recovered = {
    kind: mappedKind,
    name: stored.toolName,
    rawInput: stored.args,
  };
  const preview = extractToolPreview(recovered, recovered);
  const title =
    composeToolTitle({
      kind: mappedKind,
      title: toolLabel(recovered, recovered) ?? stored.toolName,
      command: extractShellCommand(stored.args),
      skill: extractSkillName(stored.args),
      path: preview?.path,
      query: preview?.query ?? extractSearchQuery(stored.args),
      previewKind: preview?.kind,
    }) || stored.toolName;

  if (!preview?.path && !preview?.query && isWeakToolTitle(title)) {
    return false;
  }

  live.enrichedTools.add(stored.toolCallId);
  live.onEvent({
    type: "tool.updated",
    callId: stored.toolCallId,
    title,
    kind: mappedKind,
    status: live.toolStatuses.get(stored.toolCallId),
    preview,
  });
  return true;
}

function kindFromCursorToolName(
  name: string | undefined,
  fallback?: string,
): string | undefined {
  const key = (name ?? "").toLowerCase();
  if (
    key === "grep" ||
    key === "glob" ||
    key === "rg" ||
    key.includes("search")
  ) {
    return "search";
  }
  if (key === "read") return "read";
  if (
    key === "edit" ||
    key === "write" ||
    key === "strreplace" ||
    key === "applypatch"
  ) {
    return "edit";
  }
  if (key === "shell" || key === "bash") return "execute";
  if (key === "skill" || key === "skills") return "skill";
  if (key === "agent" || key === "task" || key === "subagent") return "agent";
  if (isTaskListToolName(key)) return "tasks";
  return fallback;
}

function toolEnrichmentDelay(live: Live): number {
  let attempts = Number.POSITIVE_INFINITY;
  for (const pending of live.pendingToolEnrichments.values()) {
    attempts = Math.min(attempts, pending.attempts);
  }
  if (attempts < 4) return 100;
  if (attempts < 12) return 300;
  return 1_000;
}

function pickAutoOption(
  runtimeMode: RuntimeMode,
  kind: string | undefined,
  optionIds: string[],
): string | null {
  if (optionIds.length === 0) return null;
  const tool = (kind ?? "").toLowerCase();
  if (runtimeMode === "supervised") return null;
  if (
    runtimeMode === "auto-accept-edits" &&
    (tool === "execute" || tool === "other")
  ) {
    return null;
  }
  if (runtimeMode === "full-access") {
    return pickOption(optionIds, [
      "allow-always",
      "allow_always",
      "allow-once",
      "allow_once",
    ]);
  }
  return pickOption(optionIds, [
    "allow-once",
    "allow_once",
    "allow-always",
    "allow_always",
  ]);
}

function pickOption(optionIds: string[], preferred: string[]): string | null {
  for (const id of preferred) {
    if (optionIds.includes(id)) return id;
  }
  return null;
}

function readConfigOptions(raw: unknown): SessionConfigOption[] {
  if (!Array.isArray(raw)) return [];
  return raw.flatMap((item) => {
    const rec = asRecord(item);
    const id = String(rec?.id ?? rec?.configId ?? "").trim();
    if (!id) return [];
    return [
      {
        id,
        category: typeof rec?.category === "string" ? rec.category : undefined,
        currentValue:
          typeof rec?.currentValue === "string" ||
          typeof rec?.currentValue === "boolean"
            ? rec.currentValue
            : undefined,
      },
    ];
  });
}

function extractModelConfigId(setup: SessionSetupResult | undefined): string {
  const model = readConfigOptions(setup?.configOptions).find(
    (option) => option.category === "model" || option.id === "model",
  );
  return model?.id ?? "model";
}

function resolveSettingConfigId(
  options: SessionConfigOption[],
  settingId: string,
): string | undefined {
  const needle = settingId.trim().toLowerCase();
  const exact = options.find((option) => option.id.toLowerCase() === needle);
  if (exact) return exact.id;
  if (needle === "effort" || needle === "reasoning") {
    return options.find(
      (option) =>
        option.id === "effort" ||
        option.id === "reasoning" ||
        (option.category === "thought_level" && option.id !== "thinking"),
    )?.id;
  }
  if (needle === "fast" || needle === "fastmode") {
    return options.find(
      (option) =>
        option.id === "fast" || option.id.toLowerCase().includes("fast"),
    )?.id;
  }
  if (needle === "thinking") {
    return options.find((option) => option.id === "thinking")?.id;
  }
  if (needle === "context" || needle === "contextwindow") {
    return options.find(
      (option) => option.id === "context" || option.id === "context_size",
    )?.id;
  }
  return undefined;
}

function asRecord(value: unknown): Record<string, unknown> | null {
  if (value && typeof value === "object" && !Array.isArray(value)) {
    return value as Record<string, unknown>;
  }
  return null;
}

function toolLabel(
  update: Record<string, unknown>,
  tool: Record<string, unknown>,
): string | undefined {
  const kind = stringField(update, "kind") ?? stringField(tool, "kind");
  const named =
    humanField(update, "title") ??
    humanField(tool, "title") ??
    humanField(update, "name") ??
    humanField(tool, "name") ??
    humanField(update, "toolName") ??
    humanField(tool, "toolName") ??
    humanField(update, "tool_name") ??
    humanField(tool, "tool_name") ??
    metaLabel(update._meta ?? tool._meta);
  const fromInput = inputLabel(
    update.rawInput ??
      tool.rawInput ??
      update.raw_input ??
      tool.raw_input ??
      update.input ??
      tool.input,
  );
  const fromLocation =
    locationLabel(update.locations ?? tool.locations) ??
    contentPath(update.content ?? tool.content);

  if (named && !isWeakName(named)) return named;
  if (fromInput) return fromInput;
  if (fromLocation) return fromLocation;
  if (named) return named;
  return kindTitle(kind);
}

function toolDetail(
  update: Record<string, unknown>,
  tool: Record<string, unknown>,
): string | undefined {
  const content =
    textFromContent(update.content, "\n") ||
    textFromContent(tool.content, "\n");
  if (content.trim()) return capToolDetail(content);
  const output = update.rawOutput ?? tool.rawOutput;
  if (typeof output === "string" && output.trim()) return capToolDetail(output);
  const outputText = textFromContent(output);
  if (outputText.trim()) return capToolDetail(outputText);
  return inputLabel(
    update.rawInput ?? tool.rawInput ?? update.input ?? tool.input,
  );
}

const MAX_TOOL_DETAIL_CHARS = 8_000;

function capToolDetail(value: string): string {
  const text = value.trim();
  if (text.length <= MAX_TOOL_DETAIL_CHARS) return text;
  return `${text.slice(0, MAX_TOOL_DETAIL_CHARS)}\n…`;
}

function inputLabel(value: unknown): string | undefined {
  if (typeof value === "string" && value.trim()) {
    const text = value.trim();
    if (looksLikeCallId(text)) return undefined;
    if (text.startsWith("{") || text.startsWith("[")) {
      try {
        return inputLabel(JSON.parse(text));
      } catch {
        return text;
      }
    }
    return text;
  }
  const raw = asRecord(value);
  if (!raw) return undefined;

  const command = stringField(raw, "command");
  if (command) return command;

  const from = stringField(raw, "old_path") ?? stringField(raw, "from");
  const to =
    stringField(raw, "new_path") ??
    stringField(raw, "to") ??
    stringField(raw, "destination");
  if (from && to) return `${shortPath(from)} → ${shortPath(to)}`;

  const path =
    stringField(raw, "path") ??
    stringField(raw, "filePath") ??
    stringField(raw, "file_path") ??
    stringField(raw, "targetFile") ??
    stringField(raw, "target_file") ??
    stringField(raw, "relative_workspace_path") ??
    stringField(raw, "uri") ??
    stringField(raw, "url");
  if (path) return shortPath(path);

  const query =
    stringField(raw, "query") ??
    stringField(raw, "pattern") ??
    stringField(raw, "glob") ??
    stringField(raw, "glob_pattern") ??
    stringField(raw, "globPattern") ??
    stringField(raw, "search_term") ??
    stringField(raw, "searchTerm");
  const name = humanField(raw, "name") ?? humanField(raw, "toolName");
  if (name && query) return `${name} ${query}`;
  if (query) return query;

  const nested = inputLabel(
    raw.arguments ?? raw.args ?? raw.input ?? raw.params,
  );
  if (name && nested) return `${name} ${nested}`;
  if (nested) return nested;
  if (name) return name;
  return firstStringArg(raw);
}

function firstStringArg(raw: Record<string, unknown>): string | undefined {
  for (const [key, value] of Object.entries(raw)) {
    if (
      key === "name" ||
      key === "toolName" ||
      key === "kind" ||
      key === "type"
    ) {
      continue;
    }
    if (typeof value === "string" && value.trim() && !looksLikeCallId(value)) {
      const text = value.trim();
      if (text.length <= 200) return text;
    }
  }
  return undefined;
}

function contentPath(content: unknown): string | undefined {
  if (!Array.isArray(content)) {
    const rec = asRecord(content);
    const path = rec && stringField(rec, "path");
    return path ? shortPath(path) : undefined;
  }
  for (const item of content) {
    const rec = asRecord(item);
    const path =
      rec && (stringField(rec, "path") ?? contentPath(rec.content ?? rec.diff));
    if (path) return path;
  }
  return undefined;
}

function locationLabel(locations: unknown): string | undefined {
  if (!Array.isArray(locations)) return undefined;
  for (const item of locations) {
    const rec = asRecord(item);
    const path =
      rec &&
      (stringField(rec, "path") ??
        stringField(rec, "uri") ??
        stringField(rec, "file"));
    if (path) return shortPath(path);
  }
  return undefined;
}

function metaLabel(meta: unknown): string | undefined {
  const rec = asRecord(meta);
  if (!rec) return undefined;
  return (
    humanField(rec, "toolName") ??
    humanField(rec, "name") ??
    humanField(rec, "displayName")
  );
}

function humanField(
  rec: Record<string, unknown>,
  key: string,
): string | undefined {
  const value = stringField(rec, key);
  if (!value || looksLikeCallId(value)) return undefined;
  return value;
}

function kindTitle(kind: string | undefined): string | undefined {
  if (!kind?.trim()) return undefined;
  const key = kind.trim().toLowerCase();
  switch (key) {
    case "read":
      return "Read";
    case "edit":
      return "Edit";
    case "delete":
      return "Delete";
    case "move":
      return "Move";
    case "search":
      return "Find";
    case "execute":
    case "shell":
    case "bash":
      return "Shell";
    case "skill":
      return "Skill";
    case "think":
      return "Think";
    case "fetch":
      return "Fetch";
    case "other":
      return undefined;
    default:
      return key.replace(/^_/, "").replace(/[_-]+/g, " ");
  }
}

function isWeakName(value: string): boolean {
  return isWeakToolTitle(value);
}

function looksLikeCallId(value: string): boolean {
  const text = value.trim();
  return (
    /^(call[-_]?|tool[-_])[a-z0-9_-]+$/i.test(text) ||
    /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(text)
  );
}

function shortPath(path: string): string {
  if (/\s/.test(path)) return path;
  const parts = path.split(/[/\\]/).filter(Boolean);
  if (parts.length <= 2) return parts.join("/") || path;
  return parts.slice(-2).join("/");
}

function coerceMaybeString(
  rec: Record<string, unknown>,
  key: string,
): string | undefined {
  return stringField(rec, key);
}

function stringField(
  rec: Record<string, unknown>,
  key: string,
): string | undefined {
  const value = rec[key];
  return typeof value === "string" && value.trim() ? value : undefined;
}

function numberField(
  rec: Record<string, unknown>,
  key: string,
): number | undefined {
  const value = rec[key];
  return typeof value === "number" && Number.isFinite(value)
    ? value
    : undefined;
}

function textFromContent(content: unknown, separator = ""): string {
  if (typeof content === "string") return content;
  const rec = asRecord(content);
  if (rec && typeof rec.text === "string") return rec.text;
  if (rec && rec.content != null) {
    return textFromContent(rec.content, separator);
  }
  if (Array.isArray(content)) {
    return joinContentParts(
      content.map((item) => textFromContent(item, separator)).filter(Boolean),
      separator,
    );
  }
  return "";
}

function joinContentParts(parts: string[], separator: string): string {
  let joined = "";
  for (const part of parts) {
    if (!joined) {
      joined = part;
      continue;
    }
    const boundaryAlreadyPresent =
      !separator || /\s$/.test(joined) || /^\s/.test(part);
    joined += boundaryAlreadyPresent ? part : separator + part;
  }
  return joined;
}

export function __cursorTestReset(): void {
  liveByThread.clear();
  resumeByThread.clear();
  cancelledThreads.clear();
}
