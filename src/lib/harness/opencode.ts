import { modelContextWindow, nativeModelId } from "../models";
import type { RuntimeMode } from "../session";
import { taskListFromToolInput } from "../taskList";
import {
  execChild,
  freeHarnessPort,
  killChild,
  resolveOpenCodeBinary,
  spawnChild,
  unwatchChild,
  watchChild,
} from "./child";
import { OpenCodeClient, OpenCodeHttpError } from "./opencodeClient";
import {
  appendOpenCodeAssistantTextDelta,
  asRecord,
  buildOpenCodePermissionRules,
  compareSemver,
  contextUsedFromMessageInfo,
  detailFromToolPart,
  eventSessionId,
  isOpenCodeNotFound,
  mergeOpenCodeAssistantText,
  MINIMUM_OPENCODE_VERSION,
  KNOWN_HIDDEN_AGENTS,
  parseOpenCodeModelSlug,
  parseOpenCodeVersion,
  parseServerUrlFromOutput,
  permissionTitle,
  previewFromToolPart,
  sessionErrorMessage,
  stringField,
  textDeltaEvent,
  toOpenCodeFileParts,
  toOpenCodePermissionReply,
  toolKindFromName,
  type OpenCodePart,
} from "./opencodeProtocol";
import {
  composeToolTitle,
  extractShellCommand,
  extractSkillName,
} from "./preview";
import { streamTextDelta } from "./streamText";
import type {
  ApprovalDecision,
  CompactContextInput,
  HarnessEvent,
  HarnessSessionInput,
  SendTurnInput,
  SteerTurnInput,
} from "./types";
import {
  questionPromptTitle,
  questionsFromUnknown,
  selectedAnswerLabels,
  type UserQuestion,
  type UserQuestionReply,
} from "../userQuestion";

type PendingApproval = {
  id: string;
  resolve: (decision: ApprovalDecision) => void;
};

type PendingQuestion = {
  id: string;
  questions: UserQuestion[];
  resolve: (reply: UserQuestionReply) => void;
};

type Live = {
  client: OpenCodeClient;
  openCodeSessionId: string;
  cwd: string;
  runtimeMode: RuntimeMode;
  planning: boolean;
  onEvent: (event: HarnessEvent) => void;
  approvals: Map<number, PendingApproval>;
  questions: Map<number, PendingQuestion>;
  nextApprovalUiId: number;
  partById: Map<string, OpenCodePart>;
  emittedTextByPartId: Map<string, string>;
  messageRoleById: Map<string, "user" | "assistant" | "hidden">;
  cancelled: boolean;
  muteUpdates: boolean;
  turns: Promise<void>;
  turnDone: (() => void) | null;
  turnFailed: ((error: Error) => void) | null;
  turnEndPending: boolean;
  activeTurn: boolean;
};

type Resume = {
  sessionId: string;
  cwd: string;
};

const SERVER_TIMEOUT_MS = 30_000;
const liveByThread = new Map<string, Live>();
const resumeByThread = new Map<string, Resume>();
const cancelledThreads = new Set<string>();

let resolveOpenCodeBinaryImpl: () => Promise<{ path: string }> =
  resolveOpenCodeBinary;

/** Test seam. */
export function setOpenCodeBinaryResolver(
  fn: () => Promise<{ path: string }>,
): void {
  resolveOpenCodeBinaryImpl = fn;
}

export async function sendOpenCodeTurn(input: SendTurnInput): Promise<void> {
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

export async function compactOpenCodeContext(
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

  const model = parseOpenCodeModelSlug(nativeModelId(input.model));
  if (!model) {
    throw new Error(
      "OpenCode models use provider/model ids. Wait for the catalog to load, then pick a model.",
    );
  }
  live.onEvent = input.onEvent;
  live.turns = live.turns
    .catch(() => undefined)
    .then(async () => {
      live.cancelled = false;
      live.muteUpdates = false;
      try {
        await runCompaction(live, model);
      } catch (error) {
        if (live.cancelled) return;
        throw error;
      }
    });
  await live.turns;
}

export async function steerOpenCodeTurn(input: SteerTurnInput): Promise<void> {
  const live = liveByThread.get(input.sessionId);
  if (!live?.activeTurn) throw new Error("No active turn to steer");

  const parsed = parseOpenCodeModelSlug(nativeModelId(input.model));
  if (!parsed) {
    throw new Error(
      "OpenCode models use provider/model ids. Wait for the catalog to load, then pick a model.",
    );
  }

  const parts = [
    ...(input.text.trim()
      ? [{ type: "text" as const, text: input.text.trim() }]
      : []),
    ...toOpenCodeFileParts(input.attachments),
  ];
  if (parts.length === 0) return;

  await live.client.promptAsync({
    sessionID: live.openCodeSessionId,
    model: parsed,
    agent: input.modelSettings?.agent,
    variant: input.modelSettings?.variant,
    parts,
  });
}

export function respondOpenCodeApproval(
  sessionId: string,
  requestId: number,
  decision: ApprovalDecision,
): void {
  const live = liveByThread.get(sessionId);
  const pending = live?.approvals.get(requestId);
  if (!pending) return;
  pending.resolve(decision);
}

export function respondOpenCodeQuestion(
  sessionId: string,
  requestId: number,
  reply: UserQuestionReply,
): void {
  const live = liveByThread.get(sessionId);
  const pending = live?.questions.get(requestId);
  if (!pending) return;
  pending.resolve(reply);
}

export async function cancelOpenCodeTurn(sessionId: string): Promise<void> {
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
  await live.client.abortSession(live.openCodeSessionId);
  finishActiveTurn(live, [
    { type: "message.completed" },
    { type: "reasoning.completed" },
  ]);
}

export async function stopOpenCodeSession(sessionId: string): Promise<void> {
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
    await live.client.abortSession(live.openCodeSessionId);
    await live.client.closeEvents(sessionId);
  }
  unwatchChild(sessionId);
  await killChild(sessionId).catch(() => undefined);
}

export async function forgetOpenCodeSession(sessionId: string): Promise<void> {
  resumeByThread.delete(sessionId);
  await stopOpenCodeSession(sessionId);
}

export function bindOpenCodeSession(
  threadId: string,
  providerSessionId: string,
  cwd: string,
): void {
  const sessionId = providerSessionId.trim();
  if (!threadId || !sessionId || !cwd.trim()) return;
  resumeByThread.set(threadId, { sessionId, cwd });
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
    await stopOpenCodeSession(input.sessionId);
  }

  const resume = resumeByThread.get(input.sessionId);
  const canResume = resume != null && resume.cwd === input.cwd;
  if (resume && resume.cwd !== input.cwd) {
    resumeByThread.delete(input.sessionId);
  }

  const { path } = await resolveOpenCodeBinaryImpl();
  await assertOpenCodeVersion(path, input.cwd);

  const liveRef: { current: Live | null } = { current: null };
  let serverUrl = "";
  let serverExited: number | null | undefined;

  watchChild(
    input.sessionId,
    (line) => {
      const parsed = parseServerUrlFromOutput(line);
      if (parsed) serverUrl = parsed;
    },
    (code) => {
      serverExited = code;
      liveByThread.delete(input.sessionId);
      input.onEvent({ type: "session.ended", code });
      const live = liveRef.current;
      live?.turnFailed?.(new Error("OpenCode server exited"));
      if (live) {
        live.turnDone = null;
        live.turnFailed = null;
      }
    },
    (line) => {
      const parsed = parseServerUrlFromOutput(line);
      if (parsed) serverUrl = parsed;
    },
  );

  const port = await freeHarnessPort();
  await spawnChild(
    input.sessionId,
    path,
    ["serve", `--hostname=127.0.0.1`, `--port=${port}`],
    input.cwd,
  );

  try {
    const url = await waitForServerUrl(
      () => serverUrl,
      () => serverExited,
      SERVER_TIMEOUT_MS,
    );
    const client = new OpenCodeClient(url, input.cwd);
    const openCodeSession = await resolveSession(client, {
      resume: canResume ? resume : undefined,
      runtimeMode: input.runtimeMode,
      cwd: input.cwd,
    });

    const live: Live = {
      client,
      openCodeSessionId: openCodeSession.id,
      cwd: input.cwd,
      runtimeMode: input.runtimeMode,
      planning: input.intent === "plan",
      onEvent: input.onEvent,
      approvals: new Map(),
      questions: new Map(),
      nextApprovalUiId: 1,
      partById: new Map(),
      emittedTextByPartId: new Map(),
      messageRoleById: new Map(),
      cancelled: false,
      muteUpdates: false,
      turns: Promise.resolve(),
      turnDone: null,
      turnFailed: null,
      turnEndPending: false,
      activeTurn: false,
    };
    liveRef.current = live;
    liveByThread.set(input.sessionId, live);
    resumeByThread.set(input.sessionId, {
      sessionId: openCodeSession.id,
      cwd: input.cwd,
    });

    await client.subscribeEvents(
      input.sessionId,
      (event) => {
        if (live.muteUpdates) return;
        handleEvent(live, event);
      },
      (error) => {
        if (live.muteUpdates || live.cancelled) return;
        if (error) {
          live.onEvent({ type: "session.error", message: error });
          live.turnFailed?.(new Error(error));
        }
      },
    );

    live.onEvent({
      type: "session.providerBound",
      providerSessionId: openCodeSession.id,
    });
    live.onEvent({ type: "session.started" });
    return live;
  } catch (error) {
    await stopOpenCodeSession(input.sessionId);
    throw error;
  }
}

async function resolveSession(
  client: OpenCodeClient,
  input: {
    resume?: Resume;
    runtimeMode: RuntimeMode;
    cwd: string;
  },
) {
  const permission = buildOpenCodePermissionRules(input.runtimeMode);
  if (input.resume) {
    try {
      const adopted = await client.getSession(input.resume.sessionId);
      if (!adopted.directory || sameDirectory(adopted.directory, input.cwd)) {
        await client
          .updateSession(adopted.id, { permission })
          .catch(() => undefined);
        return adopted;
      }
      const forked = await client.forkSession(adopted.id, input.cwd);
      await client
        .updateSession(forked.id, { permission })
        .catch(() => undefined);
      return forked;
    } catch (error) {
      if (!isOpenCodeNotFound(error) && !isHttpNotFound(error)) throw error;
    }
  }
  return client.createSession({ permission });
}

async function runTurn(live: Live, input: SendTurnInput): Promise<void> {
  const parsed = parseOpenCodeModelSlug(nativeModelId(input.model));
  if (!parsed) {
    throw new Error(
      "OpenCode models use provider/model ids. Wait for the catalog to load, then pick a model.",
    );
  }
  const parts = [
    ...(input.text.trim()
      ? [{ type: "text" as const, text: input.text.trim() }]
      : []),
    ...toOpenCodeFileParts(input.attachments),
  ];
  if (parts.length === 0) return;

  const turnPromise = new Promise<void>((resolve, reject) => {
    live.turnDone = resolve;
    live.turnFailed = reject;
  });
  live.activeTurn = true;
  settlePendingTurn(live);

  try {
    await live.client.promptAsync({
      sessionID: live.openCodeSessionId,
      model: parsed,
      agent: openCodeAgentForTurn(input),
      variant: input.modelSettings?.variant,
      parts,
    });
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

async function runCompaction(
  live: Live,
  model: { providerID: string; modelID: string },
): Promise<void> {
  // Unlike prompt_async, summarize responds only after the compaction pass.
  // Keep this outside the normal turn latch: its eventual session.status=idle
  // must not become a pending completion for the next user turn.
  await live.client.summarizeSession(live.openCodeSessionId, model);
}

function handleEvent(live: Live, event: Record<string, unknown>): void {
  const payloadSessionId = eventSessionId(event);
  if (payloadSessionId && payloadSessionId !== live.openCodeSessionId) return;

  const type = typeof event.type === "string" ? event.type : "";
  const properties = asRecord(event.properties) ?? {};

  switch (type) {
    case "message.updated": {
      const info = asRecord(properties.info);
      const id = stringField(info, "id");
      const role = stringField(info, "role");
      const agent = stringField(info, "agent");
      const hidden = agent != null && KNOWN_HIDDEN_AGENTS.has(agent);
      if (id && (role === "user" || role === "assistant")) {
        live.messageRoleById.set(id, hidden ? "hidden" : role);
      }
      // A compaction assistant's usage describes the summarization call, not
      // the rebuilt context. Keep the previous meter value until a real turn
      // reports the post-compaction window level.
      if (role === "assistant" && !hidden) emitContext(live, info);
      break;
    }
    case "message.removed": {
      const messageID = stringField(properties, "messageID");
      if (messageID) live.messageRoleById.delete(messageID);
      break;
    }
    case "message.part.delta": {
      const partID = stringField(properties, "partID");
      const delta = streamTextDelta(properties.delta);
      if (!partID || !delta) break;
      const existing = live.partById.get(partID);
      if (!existing || roleForPart(live, existing) !== "assistant") break;
      const previous =
        live.emittedTextByPartId.get(partID) ?? existing.text ?? "";
      const { nextText, deltaToEmit } = appendOpenCodeAssistantTextDelta(
        previous,
        delta,
      );
      live.emittedTextByPartId.set(partID, nextText);
      if (existing.type === "text" || existing.type === "reasoning") {
        live.partById.set(partID, { ...existing, text: nextText });
      }
      const mapped = textDeltaEvent(existing, deltaToEmit);
      if (mapped) live.onEvent(mapped);
      break;
    }
    case "message.part.updated": {
      const part = parsePart(properties.part);
      if (!part) break;
      live.partById.set(part.id, part);
      if (roleForPart(live, part) === "assistant") {
        emitAssistantText(live, part);
      }
      if (part.type === "tool") emitTool(live, part);
      break;
    }
    case "permission.asked": {
      const id =
        stringField(properties, "id") ?? stringField(properties, "requestID");
      if (!id) break;
      const permission = stringField(properties, "permission") ?? "tool";
      const patterns = Array.isArray(properties.patterns)
        ? properties.patterns.filter(
            (item): item is string => typeof item === "string",
          )
        : [];
      const metadata = asRecord(properties.metadata) ?? {};
      const callId =
        stringField(properties, "callID") ??
        stringField(properties, "toolCallId") ??
        stringField(metadata, "callID") ??
        stringField(metadata, "toolCallId");
      const uiId = live.nextApprovalUiId++;
      const kind = toolKindFromName(permission);
      const preview =
        previewFromToolPart({
          id,
          type: "tool",
          tool: permission,
          state: {
            ...metadata,
            input:
              metadata.input ??
              (patterns[0] ? { path: patterns[0] } : undefined),
          },
        }) ??
        (patterns[0]
          ? previewFromToolPart({
              id,
              type: "tool",
              tool: permission,
              state: { input: { path: patterns[0], pattern: patterns[0] } },
            })
          : undefined);
      const title =
        composeToolTitle({
          kind,
          title: permissionTitle(permission, patterns),
          command:
            extractShellCommand(metadata.input) ??
            (permission === "bash" ? patterns[0] : undefined),
          skill: extractSkillName(metadata.input),
          path: preview?.path,
          query: preview?.query,
          previewKind: preview?.kind,
        }) || permissionTitle(permission, patterns);
      if (live.planning) {
        const decision =
          kind === "read" || kind === "search" ? "allow" : "deny";
        void live.client
          .replyPermission(id, toOpenCodePermissionReply(decision))
          .catch(() => undefined);
        break;
      }
      if (callId) {
        live.onEvent({
          type: "tool.updated",
          callId,
          title,
          kind,
          preview,
        });
      }
      live.onEvent({
        type: "approval.requested",
        requestId: uiId,
        title,
        kind,
        callId,
        preview,
      });
      void waitApproval(live, uiId, id);
      break;
    }
    case "question.asked": {
      const id =
        stringField(properties, "id") ?? stringField(properties, "requestID");
      if (!id) break;
      const questions = questionsFromUnknown(properties);
      const uiId = live.nextApprovalUiId++;
      live.onEvent({
        type: "question.asked",
        requestId: uiId,
        title: questionPromptTitle(questions) || "OpenCode question",
        questions,
      });
      void waitQuestion(live, uiId, id, questions);
      break;
    }
    case "session.status": {
      const status = asRecord(properties.status);
      const statusType = stringField(status, "type");
      if (statusType === "retry") {
        const message = stringField(status, "message");
        if (message) live.onEvent({ type: "status", text: message });
        break;
      }
      if (statusType === "idle" && live.activeTurn) {
        finishActiveTurn(live, [
          { type: "message.completed" },
          { type: "reasoning.completed" },
        ]);
      }
      break;
    }
    case "session.error": {
      const message = sessionErrorMessage(properties.error);
      live.onEvent({ type: "session.error", message });
      finishActiveTurn(live);
      break;
    }
    default:
      break;
  }
}

export function openCodeAgentForTurn(input: {
  intent?: SendTurnInput["intent"];
  modelSettings?: Record<string, string>;
}): string | undefined {
  if (input.intent === "plan") return "plan";
  if (input.intent === "build") return "build";
  const configured = input.modelSettings?.agent?.trim();
  return configured && configured !== "plan" ? configured : "build";
}

/**
 * OpenCode reports tokens per assistant message but not the window, so the
 * window comes from the catalog entry for the model that produced it.
 */
function emitContext(live: Live, info: Record<string, unknown> | null): void {
  const used = contextUsedFromMessageInfo(info);
  if (used === undefined) return;
  const providerID = stringField(info, "providerID");
  const modelID = stringField(info, "modelID");
  const window =
    providerID && modelID
      ? modelContextWindow(`opencode:${providerID}/${modelID}`)
      : undefined;
  live.onEvent({ type: "context", used, ...(window ? { window } : {}) });
}

function emitAssistantText(live: Live, part: OpenCodePart): void {
  const text = part.text;
  if (text === undefined) return;
  const previous = live.emittedTextByPartId.get(part.id);
  const { latestText, deltaToEmit } = mergeOpenCodeAssistantText(
    previous,
    text,
  );
  live.emittedTextByPartId.set(part.id, latestText);
  const mapped = textDeltaEvent(part, deltaToEmit);
  if (mapped) live.onEvent(mapped);
}

function emitTool(live: Live, part: OpenCodePart): void {
  const callId = part.callID ?? part.id;
  const tool = part.tool ?? "tool";
  const state = part.state ?? {};
  const status = typeof state.status === "string" ? state.status : "pending";
  const kind = toolKindFromName(tool);
  const preview = previewFromToolPart(part);
  const title =
    composeToolTitle({
      kind,
      title: (typeof state.title === "string" && state.title) || tool,
      command: extractShellCommand(state.input),
      skill: extractSkillName(state.input),
      path: preview?.path,
      query: preview?.query,
      previewKind: preview?.kind,
    }) ||
    (typeof state.title === "string" && state.title) ||
    tool;
  const detail = detailFromToolPart(part);
  const tasks = taskListFromToolInput(tool, state.input);
  if (tasks) live.onEvent({ type: "tasks.updated", items: tasks });
  if (status === "pending") {
    live.onEvent({
      type: "tool.started",
      callId,
      title,
      kind,
      status: "pending",
      preview,
    });
    return;
  }
  live.onEvent({
    type: status === "pending" ? "tool.started" : "tool.updated",
    callId,
    title,
    kind,
    status:
      status === "error"
        ? "failed"
        : status === "completed"
          ? "completed"
          : status,
    detail,
    preview,
  });
}

async function waitApproval(
  live: Live,
  uiId: number,
  id: string,
): Promise<void> {
  const decision = await new Promise<ApprovalDecision>((resolve) => {
    live.approvals.set(uiId, { id, resolve });
  });
  live.approvals.delete(uiId);
  live.onEvent({ type: "approval.resolved", requestId: uiId, decision });
  await live.client
    .replyPermission(id, toOpenCodePermissionReply(decision))
    .catch(() => undefined);
}

async function waitQuestion(
  live: Live,
  uiId: number,
  id: string,
  questions: UserQuestion[],
): Promise<void> {
  const reply = await new Promise<UserQuestionReply>((resolve) => {
    live.questions.set(uiId, { id, questions, resolve });
  });
  live.questions.delete(uiId);
  live.onEvent({
    type: "question.resolved",
    requestId: uiId,
    decision: reply.kind,
  });
  if (reply.kind !== "answered") {
    await live.client.rejectQuestion(id).catch(() => undefined);
    return;
  }
  const answers = questions.map((question) =>
    selectedAnswerLabels(question, reply),
  );
  await live.client.replyQuestion(id, answers).catch(() => undefined);
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

function parsePart(value: unknown): OpenCodePart | null {
  const rec = asRecord(value);
  const id = stringField(rec, "id");
  const type = stringField(rec, "type");
  if (!rec || !id || !type) return null;
  return {
    id,
    type,
    messageID: stringField(rec, "messageID"),
    callID: stringField(rec, "callID"),
    tool: stringField(rec, "tool"),
    text: typeof rec.text === "string" ? rec.text : undefined,
    time: asRecord(rec.time) as OpenCodePart["time"],
    state: asRecord(rec.state) ?? undefined,
  };
}

function roleForPart(
  live: Live,
  part: Pick<OpenCodePart, "messageID" | "type">,
): "assistant" | "user" | "hidden" | undefined {
  if (part.messageID) {
    const known = live.messageRoleById.get(part.messageID);
    if (known) return known;
  }
  return part.type === "tool" ||
    part.type === "text" ||
    part.type === "reasoning"
    ? "assistant"
    : undefined;
}

function sameDirectory(left: string, right: string): boolean {
  const normalize = (value: string) =>
    value.replace(/\/+$/, "").replace(/\\/g, "/");
  return normalize(left) === normalize(right);
}

function isHttpNotFound(error: unknown): boolean {
  return error instanceof OpenCodeHttpError && error.status === 404;
}

async function assertOpenCodeVersion(path: string, cwd: string): Promise<void> {
  const output = await execChild(path, ["--version"], cwd).catch(() => "");
  const version = parseOpenCodeVersion(output);
  if (!version) {
    throw new Error(
      `Unable to determine OpenCode version. MonoCode requires v${MINIMUM_OPENCODE_VERSION} or newer.`,
    );
  }
  if (compareSemver(version, MINIMUM_OPENCODE_VERSION) < 0) {
    throw new Error(
      `OpenCode v${version} is too old. Upgrade to v${MINIMUM_OPENCODE_VERSION} or newer.`,
    );
  }
}

function waitForServerUrl(
  read: () => string,
  exited: () => number | null | undefined,
  timeoutMs: number,
): Promise<string> {
  return new Promise((resolve, reject) => {
    const started = Date.now();
    const tick = () => {
      const url = read();
      if (url) {
        resolve(url);
        return;
      }
      if (exited() !== undefined) {
        reject(
          new Error(
            `OpenCode server exited before startup completed (code: ${String(exited())}).`,
          ),
        );
        return;
      }
      if (Date.now() - started >= timeoutMs) {
        reject(new Error("Timed out waiting for OpenCode server"));
        return;
      }
      setTimeout(tick, 50);
    };
    tick();
  });
}
