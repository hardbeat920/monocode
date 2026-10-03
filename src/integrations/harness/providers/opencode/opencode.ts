import {
  modelContextWindow,
  nativeModelId,
} from "../../../../features/sessions/model/models";
import type {
  RuntimeMode,
  TurnMetrics,
} from "../../../../features/sessions/model/session";
import { taskListFromToolInput } from "../../../../features/sessions/model/taskList";
import {
  closeHarnessSse,
  execChild,
  freeHarnessPort,
  killChild,
  resolveOpenCodeBinary,
  spawnChild,
  unwatchChild,
  watchChild,
} from "../../core/child";
import {
  OpenCodeClient,
  OpenCodeHttpError,
  type OpenCodeMessage,
} from "./opencodeClient";
import {
  appendOpenCodeAssistantTextDelta,
  asRecord,
  buildOpenCodePermissionRules,
  isSupportedOpenCodeVersion,
  managedOpenCodeConfig,
  verifyManagedOpenCodePolicy,
  nextOpenCodeMessageId,
  unsupportedOpenCodeVersionMessage,
  contextUsedFromMessageInfo,
  turnMetricsFromMessageInfo,
  detailFromToolPart,
  eventSessionId,
  isOpenCodeNotFound,
  openCodeChildSessionId,
  mergeOpenCodeAssistantText,
  KNOWN_HIDDEN_AGENTS,
  parseOpenCodeModelSlug,
  parseOpenCodeVersion,
  parseOpenCodeToolOutputGlob,
  parseServerUrlFromOutput,
  permissionTitle,
  previewFromToolPart,
  sessionErrorMessage,
  stringField,
  toOpenCodePromptParts,
  toOpenCodePermissionReply,
  toolKindFromName,
  type OpenCodePart,
} from "./opencodeProtocol";
import {
  composeToolTitle,
  extractShellCommand,
  extractSkillName,
} from "../../core/preview";
import { streamTextDelta } from "../../core/streamText";
import type {
  ApprovalDecision,
  CompactContextInput,
  HarnessEvent,
  HarnessSessionInput,
  RewindLastTurnInput,
  RewindLastTurnResult,
  SendTurnInput,
  SteerTurnInput,
} from "../../core/types";
import {
  questionPromptTitle,
  questionsFromUnknown,
  selectedAnswerLabels,
  type UserQuestion,
  type UserQuestionReply,
} from "../../../../features/sessions/model/userQuestion";

type PendingApproval = {
  id: string;
  resolve: (decision: ApprovalDecision) => void;
};

type PendingQuestion = {
  id: string;
  questions: UserQuestion[];
  resolve: (reply: UserQuestionReply) => void;
};

type ActivePrompt = {
  messageIDs: Set<string>;
  assistantIDs: Set<string>;
  accepted: boolean;
  observed: boolean;
  idleSeen: boolean;
  checking: Promise<void> | null;
  pendingError?: {
    message: string;
    name?: string;
    progressAt: number;
    graceMs: number;
  };
  errorTimer?: ReturnType<typeof setTimeout>;
};

type Live = {
  threadId: string;
  activeAgent: string | undefined;
  prompt: ActivePrompt | null;
  compacting: boolean;
  compactionError?: string;
  client: OpenCodeClient;
  openCodeSessionId: string;
  cwd: string;
  runtimeMode: RuntimeMode;
  planning: boolean;
  onEvent: (event: HarnessEvent) => void;
  approvals: Map<number, PendingApproval>;
  questions: Map<number, PendingQuestion>;
  visibleQuestionId: number | null;
  nextApprovalUiId: number;
  sessionParentById: Map<string, string | undefined>;
  /** Child session id -> the agent tool row that spawned it. */
  subagentSessions: Map<string, string>;
  subagentModels: Map<string, string>;
  /** Child parts that arrived before their row was known. */
  pendingSubagent: Map<string, OpenCodePart[]>;
  partById: Map<string, OpenCodePart>;
  emittedTextByPartId: Map<string, string>;
  messageRoleById: Map<string, "user" | "assistant" | "hidden">;
  turnMetricsByMessageId: Map<string, TurnMetrics>;
  cancelled: boolean;
  muteUpdates: boolean;
  turns: Promise<void>;
  turnDone: (() => void) | null;
  turnFailed: ((error: Error) => void) | null;
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
const openingThreads = new Map<string, number>();
const lifecycleByThread = new Map<string, Promise<void>>();

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
  if (cancelledThreads.delete(input.sessionId)) {
    await stopOwnedLive(input.sessionId, live);
    return;
  }

  live.onEvent = input.onEvent;
  live.runtimeMode = input.runtimeMode;
  live.planning = input.intent === "plan";
  live.turns = live.turns
    .catch(() => undefined)
    .then(async () => {
      if (!canRunQueuedOperation(live)) return;
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
  if (cancelledThreads.delete(input.sessionId)) {
    await stopOwnedLive(input.sessionId, live);
    return;
  }

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
      if (!canRunQueuedOperation(live)) return;
      try {
        await runCompaction(live, model);
      } catch (error) {
        if (live.cancelled) return;
        throw error;
      }
    });
  await live.turns;
}

export async function rewindOpenCodeLastTurn(
  input: RewindLastTurnInput,
): Promise<RewindLastTurnResult> {
  let live: Live;
  try {
    live = await ensureLive(input);
  } catch (error) {
    cancelledThreads.delete(input.sessionId);
    throw error;
  }
  if (cancelledThreads.delete(input.sessionId)) {
    await stopOwnedLive(input.sessionId, live);
    return { submitted: false };
  }

  live.onEvent = input.onEvent;
  await live.turns.catch(() => undefined);
  if (!canRunQueuedOperation(live)) return { submitted: false };
  if (live.activeTurn) {
    throw new Error("Stop the current turn before editing the last message");
  }

  const messageID = await latestOpenCodeUserMessageId(live);
  if (!canRunQueuedOperation(live)) return { submitted: false };
  await live.client.revertSession(live.openCodeSessionId, messageID);
  return { submitted: false };
}

function canRunQueuedOperation(live: Live): boolean {
  if (live.cancelled) return false;
  if (liveByThread.get(live.threadId) !== live || live.muteUpdates)
    throw new Error(
      "OpenCode session ended before this operation could start. Retry the request.",
    );
  return true;
}

async function latestOpenCodeUserMessageId(live: Live): Promise<string> {
  const messages = await live.client.getMessages(live.openCodeSessionId);
  const candidates = messages.flatMap((message) => {
    const info = asRecord(message.info);
    if (stringField(info, "role") !== "user") return [];
    const parts = message.parts ?? [];
    if (
      parts.length > 0 &&
      !parts.some((part) => {
        const record = asRecord(part);
        return (
          record?.synthetic !== true &&
          ["text", "file"].includes(stringField(record, "type") ?? "")
        );
      })
    )
      return [];
    const id = stringField(info, "id");
    if (!id) return [];
    const created = asRecord(info?.time)?.created;
    return [
      {
        id,
        created:
          typeof created === "number" && Number.isFinite(created)
            ? created
            : undefined,
      },
    ];
  });
  const timestamped = candidates.filter(
    (candidate): candidate is { id: string; created: number } =>
      candidate.created !== undefined,
  );
  const latest =
    candidates.length > 0 && timestamped.length === candidates.length
      ? timestamped.reduce((current, candidate) =>
          candidate.created >= current.created ? candidate : current,
        )
      : candidates[candidates.length - 1];
  if (!latest) {
    throw new Error("OpenCode did not expose the last user message");
  }
  return latest.id;
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

  const parts = toOpenCodePromptParts(input.text, input.attachments);
  if (parts.length === 0) return;

  const messageID = nextOpenCodeMessageId();
  const prompt = live.prompt;
  prompt?.messageIDs.add(messageID);
  if (prompt?.pendingError) prompt.pendingError.progressAt = Date.now();
  try {
    await live.client.promptAsync({
      sessionID: live.openCodeSessionId,
      model: parsed,
      agent: live.activeAgent,
      messageID,
      variant: input.modelSettings?.variant,
      parts,
    });
  } catch (error) {
    prompt?.messageIDs.delete(messageID);
    throw error;
  }
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
  if (openingThreads.has(sessionId)) cancelledThreads.add(sessionId);
  await withLifecycle(sessionId, () => cancelLive(sessionId));
}

async function cancelLive(sessionId: string): Promise<void> {
  const live = liveByThread.get(sessionId);
  if (!live) {
    if (resumeByThread.has(sessionId) && !openingThreads.has(sessionId)) return;
    cancelledThreads.add(sessionId);
    return;
  }
  live.cancelled = true;
  live.muteUpdates = true;
  liveByThread.delete(sessionId);
  for (const pending of live.approvals.values()) pending.resolve("deny");
  live.approvals.clear();
  for (const pending of live.questions.values())
    pending.resolve({ kind: "skipped" });
  live.questions.clear();
  let failure: unknown;
  try {
    await live.client.abortSession(live.openCodeSessionId);
  } catch (error) {
    failure = error;
    live.onEvent({
      type: "session.error",
      message: `Could not confirm OpenCode cancellation: ${error instanceof Error ? error.message : String(error)}`,
    });
  } finally {
    try {
      await live.client.closeEvents(sessionId);
    } finally {
      unwatchChild(sessionId);
      try {
        await killChild(sessionId);
      } finally {
        finishActiveTurn(live, [
          { type: "message.completed" },
          { type: "reasoning.completed" },
        ]);
      }
    }
  }
  if (failure) throw failure;
}

export async function stopOpenCodeSession(sessionId: string): Promise<void> {
  cancelledThreads.delete(sessionId);
  await withLifecycle(sessionId, () => stopLive(sessionId));
}

async function stopOwnedLive(sessionId: string, live: Live): Promise<void> {
  await withLifecycle(sessionId, async () => {
    if (liveByThread.get(sessionId) === live) await stopLive(sessionId);
  });
}

async function stopLive(sessionId: string): Promise<void> {
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
    await live.client
      .abortSession(live.openCodeSessionId)
      .catch(() => undefined);
    await live.client.closeEvents(sessionId).catch(() => undefined);
  } else {
    // A stream or server that ended on its own already dropped `live`, but
    // its SSE handlers still hold it until the stream is closed.
    await closeHarnessSse(sessionId).catch(() => undefined);
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
  openingThreads.set(
    input.sessionId,
    (openingThreads.get(input.sessionId) ?? 0) + 1,
  );
  try {
    return await withLifecycle(input.sessionId, () => startLive(input));
  } finally {
    const count = (openingThreads.get(input.sessionId) ?? 1) - 1;
    if (count > 0) openingThreads.set(input.sessionId, count);
    else openingThreads.delete(input.sessionId);
  }
}

async function startLive(input: HarnessSessionInput): Promise<Live> {
  const existing = liveByThread.get(input.sessionId);
  const planning = input.intent === "plan";
  if (
    existing &&
    !existing.muteUpdates &&
    !existing.cancelled &&
    existing.cwd === input.cwd &&
    existing.runtimeMode === input.runtimeMode &&
    existing.planning === planning
  ) {
    existing.onEvent = input.onEvent;
    return existing;
  }
  if (existing) {
    if (existing.cwd !== input.cwd) resumeByThread.delete(input.sessionId);
    await stopLive(input.sessionId);
  }

  const resume = resumeByThread.get(input.sessionId);
  const canResume = resume != null && resume.cwd === input.cwd;
  if (resume && resume.cwd !== input.cwd) {
    resumeByThread.delete(input.sessionId);
  }

  const { path } = await resolveOpenCodeBinaryImpl();
  await assertOpenCodeVersion(path, input.cwd);
  const agents = await execChild(
    path,
    ["agent", "list"],
    input.cwd,
    "opencode",
  );
  const toolOutputGlob =
    planning || input.runtimeMode !== "full-access"
      ? parseOpenCodeToolOutputGlob(
          await execChild(path, ["debug", "paths"], input.cwd, "opencode"),
        )
      : undefined;
  const policy = managedOpenCodeConfig(
    agents,
    input.runtimeMode,
    planning,
    toolOutputGlob,
  );

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
      const live = liveRef.current;
      if (live && liveByThread.get(input.sessionId) === live)
        liveByThread.delete(input.sessionId);
      if (!live?.muteUpdates) {
        (live?.onEvent ?? input.onEvent)({ type: "session.ended", code });
      }
      if (live) live.muteUpdates = true;
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
    undefined,
    "opencode",
    { OPENCODE_CONFIG_CONTENT: JSON.stringify(policy) },
  );

  try {
    const url = await waitForServerUrl(
      () => serverUrl,
      () => serverExited,
      SERVER_TIMEOUT_MS,
    );
    const client = new OpenCodeClient(url, input.cwd);
    if (planning || input.runtimeMode !== "full-access") {
      const agents = await client.getAgents();
      const config = await client.getConfig();
      verifyManagedOpenCodePolicy(
        agents,
        config,
        input.runtimeMode,
        planning,
        toolOutputGlob,
      );
    }
    const openCodeSession = await resolveSession(client, {
      resume: canResume ? resume : undefined,
      runtimeMode: input.runtimeMode,
      planning,
      cwd: input.cwd,
      toolOutputGlob,
    });
    if (canResume) {
      await repairUnsupportedFileTurn(client, openCodeSession.id).catch(
        (error: unknown) =>
          console.debug("[monocode] opencode attachment recovery", error),
      );
    }

    const live: Live = {
      threadId: input.sessionId,
      activeAgent: undefined,
      prompt: null,
      compacting: false,
      client,
      openCodeSessionId: openCodeSession.id,
      cwd: input.cwd,
      runtimeMode: input.runtimeMode,
      planning: input.intent === "plan",
      onEvent: input.onEvent,
      approvals: new Map(),
      questions: new Map(),
      visibleQuestionId: null,
      nextApprovalUiId: 1,
      sessionParentById: new Map(),
      subagentSessions: new Map(),
      subagentModels: new Map(),
      pendingSubagent: new Map(),
      partById: new Map(),
      emittedTextByPartId: new Map(),
      messageRoleById: new Map(),
      turnMetricsByMessageId: new Map(),
      cancelled: false,
      muteUpdates: false,
      turns: Promise.resolve(),
      turnDone: null,
      turnFailed: null,
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
        const turn = live.turnDone;
        void handleEvent(live, event).catch((error: unknown) => {
          if (live.muteUpdates || live.turnDone !== turn) return;
          // Failed ancestry lookups or replies must end the turn visibly;
          // otherwise a child can remain blocked on an unanswered request.
          live.onEvent({
            type: "session.error",
            message: `Could not route OpenCode event: ${error instanceof Error ? error.message : String(error)}`,
          });
          finishActiveTurn(live);
        });
      },
      (error) => {
        if (live.muteUpdates || live.cancelled) return;
        const message =
          error?.trim() || "OpenCode event stream ended unexpectedly.";
        // prompt_async has no response body to await; the SSE stream is its
        // only completion channel. Reusing a Live after this point accepts the
        // next prompt but can never observe it, which looks like a dead thread.
        const failed = live.turnFailed;
        live.turnDone = null;
        live.turnFailed = null;
        live.muteUpdates = true;
        for (const pending of live.approvals.values()) pending.resolve("deny");
        live.approvals.clear();
        for (const pending of live.questions.values())
          pending.resolve({ kind: "skipped" });
        live.questions.clear();
        void withLifecycle(input.sessionId, async () => {
          if (liveByThread.get(input.sessionId) !== live) return;
          liveByThread.delete(input.sessionId);
          unwatchChild(input.sessionId);
          await killChild(input.sessionId);
        })
          .catch(() => undefined)
          .then(() => {
            if (failed) {
              failed(new Error(message));
            } else {
              live.onEvent({ type: "session.error", message });
            }
          });
      },
    );

    live.onEvent({
      type: "session.providerBound",
      providerSessionId: openCodeSession.id,
    });
    live.onEvent({ type: "session.started" });
    return live;
  } catch (error) {
    await stopLive(input.sessionId);
    throw error;
  }
}

async function resolveSession(
  client: OpenCodeClient,
  input: {
    resume?: Resume;
    runtimeMode: RuntimeMode;
    planning: boolean;
    cwd: string;
    toolOutputGlob?: string;
  },
) {
  const permission = buildOpenCodePermissionRules(
    input.runtimeMode,
    input.planning,
    input.toolOutputGlob,
  );
  if (input.resume) {
    try {
      const adopted = await client.getSession(input.resume.sessionId);
      if (!adopted.directory || sameDirectory(adopted.directory, input.cwd)) {
        await client.updateSession(adopted.id, { permission });
        return adopted;
      }
      const forked = await client.forkSession(adopted.id, input.cwd);
      await client.updateSession(forked.id, { permission });
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
  const parts = toOpenCodePromptParts(input.text, input.attachments);
  if (parts.length === 0) return;

  const turnPromise = new Promise<void>((resolve, reject) => {
    live.turnDone = resolve;
    live.turnFailed = reject;
  });
  const messageID = nextOpenCodeMessageId();
  live.prompt = {
    messageIDs: new Set([messageID]),
    assistantIDs: new Set(),
    accepted: false,
    observed: false,
    idleSeen: false,
    checking: null,
  };
  live.activeTurn = true;
  live.activeAgent = openCodeAgentForTurn(input);
  live.turnMetricsByMessageId.clear();

  try {
    await live.client.promptAsync({
      sessionID: live.openCodeSessionId,
      messageID,
      model: parsed,
      agent: live.activeAgent,
      variant: input.modelSettings?.variant,
      parts,
    });
    input.onAccepted?.();
    if (live.prompt) live.prompt.accepted = true;
    if (live.prompt?.pendingError)
      scheduleBufferedErrorCheck(live, live.prompt, 0);
    if (live.prompt?.idleSeen) await reconcileIdlePrompt(live);
    await turnPromise;
  } catch (error) {
    if (live.cancelled) return;
    live.onEvent({
      type: "session.error",
      message: error instanceof Error ? error.message : String(error),
    });
    throw error;
  } finally {
    if (live.prompt?.errorTimer) clearTimeout(live.prompt.errorTimer);
    live.activeTurn = false;
    live.prompt = null;
    live.turnDone = null;
    live.turnFailed = null;
  }
}

async function runCompaction(
  live: Live,
  model: { providerID: string; modelID: string },
): Promise<void> {
  const before = new Set(
    (await live.client.getMessages(live.openCodeSessionId)).map((message) =>
      stringField(asRecord(message.info), "id"),
    ),
  );
  if (!canRunQueuedOperation(live)) return;
  live.compacting = true;
  live.compactionError = undefined;
  try {
    await live.client.summarizeSession(live.openCodeSessionId, model);
    const messages = await live.client.getMessages(live.openCodeSessionId);
    const failed = messages.find((message) => {
      const info = asRecord(message.info);
      return !before.has(stringField(info, "id")) && info?.error;
    });
    if (failed || live.compactionError)
      throw new Error(
        failed ? sessionErrorMessage(failed.info?.error) : live.compactionError,
      );
  } finally {
    live.compacting = false;
    live.compactionError = undefined;
  }
}

async function handleEvent(
  live: Live,
  event: Record<string, unknown>,
): Promise<void> {
  const type = typeof event.type === "string" ? event.type : "";
  const properties = asRecord(event.properties) ?? {};
  // Session lifecycle events establish ancestry, including nested subagents.
  // Record them before applying the parent transcript's session filter.
  if (type === "session.created" || type === "session.updated") {
    const info = asRecord(properties.info);
    const id = stringField(info, "id");
    if (id) {
      const parentId = stringField(info, "parentID");
      live.sessionParentById.set(id, parentId);
    }
    return;
  }

  const payloadSessionId = eventSessionId(event);
  if (payloadSessionId && payloadSessionId !== live.openCodeSessionId) {
    if (
      type === "message.updated" ||
      type === "message.part.updated" ||
      type === "message.part.delta"
    ) {
      handleSubagentEvent(live, payloadSessionId, type, properties);
      return;
    }
    // Only blocking interactions are forwarded otherwise. In particular, a
    // child's idle/error event must never finish the parent's active turn.
    if (type !== "permission.asked" && type !== "question.asked") return;
    const turn = live.turnDone;
    if (!(await isDescendantSession(live, payloadSessionId))) return;
    if (live.muteUpdates || live.turnDone !== turn) return;
  }

  switch (type) {
    case "message.updated": {
      const info = asRecord(properties.info);
      const id = stringField(info, "id");
      const role = stringField(info, "role");
      const agent = stringField(info, "agent");
      const hidden = agent != null && KNOWN_HIDDEN_AGENTS.has(agent);
      if (id && (role === "user" || role === "assistant")) {
        live.messageRoleById.set(id, hidden ? "hidden" : role);
        if (role === "user" && live.prompt?.messageIDs.has(id)) {
          live.prompt.observed = true;
        }
        if (
          role === "assistant" &&
          live.prompt?.messageIDs.has(stringField(info, "parentID") ?? "")
        )
          live.prompt.assistantIDs.add(id);
        if (
          live.prompt?.pendingError &&
          (live.prompt.messageIDs.has(id) || live.prompt.assistantIDs.has(id))
        )
          live.prompt.pendingError.progressAt = Date.now();
        for (const part of live.partById.values()) {
          if (
            part.messageID === id &&
            roleForPart(live, part) === "assistant"
          ) {
            emitAssistantText(live, part);
            if (part.type === "tool" && roleForPart(live, part) === "assistant")
              emitTool(live, part);
          }
        }
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
      if (
        !existing ||
        typeof existing.time?.end === "number" ||
        roleForPart(live, existing) !== "assistant"
      )
        break;
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
      if (deltaToEmit)
        emitAssistantSnapshot(live, { ...existing, text: nextText });
      break;
    }
    case "message.part.updated": {
      const part = parsePart(properties.part);
      if (!part) break;
      live.partById.set(part.id, part);
      if (
        live.prompt?.pendingError &&
        part.messageID &&
        (live.prompt.messageIDs.has(part.messageID) ||
          live.prompt.assistantIDs.has(part.messageID))
      )
        live.prompt.pendingError.progressAt = Date.now();
      if (roleForPart(live, part) === "assistant") {
        emitAssistantText(live, part);
      }
      if (part.type === "tool" && roleForPart(live, part) === "assistant")
        emitTool(live, part);
      break;
    }
    case "permission.asked": {
      const id =
        stringField(properties, "id") ?? stringField(properties, "requestID");
      if (!id) break;
      if ([...live.approvals.values()].some((pending) => pending.id === id))
        break;
      const permission = stringField(properties, "permission") ?? "tool";
      const patterns = Array.isArray(properties.patterns)
        ? properties.patterns.filter(
            (item): item is string => typeof item === "string",
          )
        : [];
      const metadata = asRecord(properties.metadata) ?? {};
      const callId =
        stringField(asRecord(properties.tool), "callID") ??
        stringField(properties, "callID") ??
        stringField(properties, "toolCallId") ??
        stringField(metadata, "callID") ??
        stringField(metadata, "toolCallId");
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
          kind === "read" ||
          kind === "search" ||
          (permission === "task" &&
            Array.isArray(properties.patterns) &&
            properties.patterns.length > 0 &&
            properties.patterns.every((pattern) => pattern === "explore"))
            ? "allow"
            : "deny";
        await live.client.replyPermission(
          id,
          toOpenCodePermissionReply(decision),
        );
        break;
      }
      if (live.runtimeMode === "full-access") {
        await live.client.replyPermission(id, "once");
        break;
      }
      const uiId = live.nextApprovalUiId++;
      const pending = waitApproval(live, uiId, id);
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
      await pending;
      break;
    }
    case "question.asked": {
      const id =
        stringField(properties, "id") ?? stringField(properties, "requestID");
      if (!id) break;
      if ([...live.questions.values()].some((pending) => pending.id === id))
        break;
      const questions = questionsFromUnknown(properties);
      const uiId = live.nextApprovalUiId++;
      const pending = waitQuestion(live, uiId, id, questions);
      showNextQuestion(live);
      await pending;
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
        if (live.prompt) live.prompt.idleSeen = true;
        await reconcileIdlePrompt(live);
      }
      break;
    }
    case "session.error": {
      const message = sessionErrorMessage(properties.error);
      const errorName = stringField(asRecord(properties.error), "name");
      if (live.compacting) {
        live.compactionError = message;
        break;
      }
      if (!live.activeTurn || !live.prompt) break;
      const setupFailure =
        /^(Agent|Model) not found:/.test(message) ||
        ["ProviderModelNotFoundError", "ModelNotFoundError"].includes(
          errorName ?? "",
        );
      live.prompt.pendingError = {
        message,
        name: errorName,
        progressAt: live.prompt.pendingError?.progressAt ?? Date.now(),
        graceMs: setupFailure
          ? 50
          : (live.prompt.pendingError?.graceMs ?? SERVER_TIMEOUT_MS),
      };
      if (errorName === "ContextOverflowError") {
        live.onEvent({
          type: "status",
          text: "OpenCode is compacting context after the provider rejected its size.",
        });
      } else live.onEvent({ type: "status", text: message });
      scheduleBufferedErrorCheck(live, live.prompt, 0);
      break;
    }
    default:
      break;
  }
}

async function isDescendantSession(
  live: Live,
  sessionId: string,
): Promise<boolean> {
  const visited = new Set<string>();
  let current: string | undefined = sessionId;
  while (current && !visited.has(current)) {
    if (current === live.openCodeSessionId) return true;
    visited.add(current);
    if (!live.sessionParentById.has(current)) {
      // Resumed children may predate the SSE subscription. Resolve their
      // ancestry from the server instead of relying on session.created alone.
      const session = await live.client.getSession(current);
      live.sessionParentById.set(current, session.parentID);
    }
    current = live.sessionParentById.get(current);
  }
  return false;
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
  const metrics = turnMetricsFromMessageInfo(info);
  const messageId = stringField(info, "id");
  if (metrics && messageId) live.turnMetricsByMessageId.set(messageId, metrics);
  if (metrics && !messageId) {
    live.onEvent({ type: "turn.metrics", ...metrics });
  }
  const aggregate = [
    ...live.turnMetricsByMessageId.values(),
  ].reduce<TurnMetrics>(
    (total, current) => ({
      inputTokens: (total.inputTokens ?? 0) + (current.inputTokens ?? 0),
      outputTokens: (total.outputTokens ?? 0) + (current.outputTokens ?? 0),
      cacheReadTokens:
        (total.cacheReadTokens ?? 0) + (current.cacheReadTokens ?? 0),
      cacheWriteTokens:
        (total.cacheWriteTokens ?? 0) + (current.cacheWriteTokens ?? 0),
    }),
    {},
  );
  const aggregateInput =
    (aggregate.inputTokens ?? 0) +
    (aggregate.cacheReadTokens ?? 0) +
    (aggregate.cacheWriteTokens ?? 0);
  const hasAggregate = Object.values(aggregate).some(
    (value) => typeof value === "number" && value > 0,
  );
  if (hasAggregate) {
    aggregate.cacheHitPercent =
      aggregateInput > 0
        ? ((aggregate.cacheReadTokens ?? 0) / aggregateInput) * 100
        : undefined;
    live.onEvent({ type: "turn.metrics", ...aggregate });
  }
  if (used === undefined) return;
  const providerID = stringField(info, "providerID");
  const modelID = stringField(info, "modelID");
  const window =
    providerID && modelID
      ? modelContextWindow(`opencode:${providerID}/${modelID}`, live.cwd)
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
    typeof part.time?.end === "number",
  );
  live.emittedTextByPartId.set(part.id, latestText);
  if (
    deltaToEmit ||
    previous !== latestText ||
    typeof part.time?.end === "number"
  )
    emitAssistantSnapshot(live, { ...part, text: latestText });
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
    if (kind === "agent") trackSubagentRow(live, callId, part);
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
    detail:
      detail ??
      (status === "error"
        ? kind === "agent"
          ? "Subagent failed."
          : "Tool failed."
        : undefined),
    preview,
  });
  // Bind after creating the parent block: replayed steps need an owner.
  if (kind === "agent") trackSubagentRow(live, callId, part);
}

/** How many parts an unidentified child may bank before its row is known. */
const MAX_PENDING_SUBAGENT = 64;

/**
 * Task metadata names the child session. Arrival order is not an identity:
 * concurrent tasks can create their sessions in any order.
 */
function trackSubagentRow(
  live: Live,
  callId: string,
  part: OpenCodePart,
): void {
  const named = openCodeChildSessionId(part);
  if (named && named !== live.openCodeSessionId) {
    bindSubagentSession(live, named, callId);
  }
}

function bindSubagentSession(
  live: Live,
  sessionId: string,
  callId: string,
): void {
  if (live.subagentSessions.get(sessionId) === callId) return;
  live.subagentSessions.set(sessionId, callId);
  const model = live.subagentModels.get(sessionId);
  if (model)
    live.onEvent({
      type: "tool.updated",
      callId,
      kind: "agent",
      agentModel: model,
    });
  const backlog = live.pendingSubagent.get(sessionId);
  live.pendingSubagent.delete(sessionId);
  for (const part of backlog ?? [])
    emitSubagentStep(live, callId, sessionId, part);
}

function handleSubagentEvent(
  live: Live,
  sessionId: string,
  type: string,
  properties: Record<string, unknown>,
): void {
  // The server broadcasts other sessions too. Only retain known descendants.
  let ancestor: string | undefined = sessionId;
  const visited = new Set<string>();
  while (ancestor && !visited.has(ancestor)) {
    if (
      ancestor === live.openCodeSessionId ||
      live.subagentSessions.has(ancestor)
    )
      break;
    visited.add(ancestor);
    ancestor = live.sessionParentById.get(ancestor);
  }
  if (!ancestor || visited.has(ancestor)) return;
  if (type === "message.updated") {
    const info = asRecord(properties.info);
    const id = stringField(info, "id");
    const role = stringField(info, "role");
    const agent = stringField(info, "agent");
    const model = stringField(info, "modelID");
    // Nested agents share the outer trail, but have their own model.
    if (
      role === "assistant" &&
      model &&
      !(agent && KNOWN_HIDDEN_AGENTS.has(agent)) &&
      live.sessionParentById.get(sessionId) === live.openCodeSessionId
    ) {
      live.subagentModels.set(sessionId, model);
      const callId = live.subagentSessions.get(sessionId);
      if (callId)
        live.onEvent({
          type: "tool.updated",
          callId,
          kind: "agent",
          agentModel: model,
        });
    }
    if (id && (role === "user" || role === "assistant")) {
      live.messageRoleById.set(
        id,
        agent && KNOWN_HIDDEN_AGENTS.has(agent) ? "hidden" : role,
      );
      // Message metadata may follow the first part on a resumed stream.
      for (const part of live.partById.values()) {
        if (part.messageID === id) mirrorSubagentPart(live, sessionId, part);
      }
    }
    return;
  }
  let part =
    type === "message.part.updated" ? parsePart(properties.part) : null;
  if (type === "message.part.delta") {
    const id = stringField(properties, "partID");
    const existing = id ? live.partById.get(id) : undefined;
    const delta = streamTextDelta(properties.delta);
    if (
      existing &&
      typeof existing.time?.end !== "number" &&
      delta &&
      (existing.type === "text" || existing.type === "reasoning")
    ) {
      part = { ...existing, text: (existing.text ?? "") + delta };
    }
  }
  if (!part) return;
  live.partById.set(part.id, part);
  mirrorSubagentPart(live, sessionId, part);
}

/**
 * One thing a subagent did, mirrored onto its row. Until the child's session
 * is tied to a row the part is kept, because a task's opening moves arrive
 * before OpenCode reports the session it created for them.
 */
function mirrorSubagentPart(
  live: Live,
  sessionId: string,
  part: OpenCodePart,
): void {
  const callId = live.subagentSessions.get(sessionId);
  if (callId) {
    emitSubagentStep(live, callId, sessionId, part);
    return;
  }
  if (part.type !== "tool" && part.type !== "text" && part.type !== "reasoning")
    return;
  const backlog = live.pendingSubagent.get(sessionId) ?? [];
  const index = backlog.findIndex((entry) => entry.id === part.id);
  if (index >= 0) backlog[index] = part;
  else backlog.push(part);
  if (backlog.length > MAX_PENDING_SUBAGENT) backlog.shift();
  if (!live.pendingSubagent.has(sessionId) && live.pendingSubagent.size >= 32) {
    live.pendingSubagent.delete(live.pendingSubagent.keys().next().value!);
  }
  live.pendingSubagent.set(sessionId, backlog);
}

function emitSubagentStep(
  live: Live,
  callId: string,
  sessionId: string,
  part: OpenCodePart,
): void {
  if (part.messageID && !live.messageRoleById.has(part.messageID)) return;
  if (roleForPart(live, part) !== "assistant") return;
  if (part.type === "text" || part.type === "reasoning") {
    const text = part.text?.trim();
    if (!text) return;
    live.onEvent({
      type: "agent.step",
      callId,
      stepId: `${sessionId}:${part.id}`,
      kind: part.type === "reasoning" ? "reasoning" : "message",
      text,
    });
    return;
  }
  if (part.type !== "tool") return;
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
  const failed = status === "error";
  live.onEvent({
    type: "agent.step",
    callId,
    stepId: `${sessionId}:${part.callID ?? part.id}`,
    kind: "tool",
    text: title,
    toolKind: kind,
    status: failed
      ? "failed"
      : status === "completed"
        ? "completed"
        : "in_progress",
    // Only a failure earns detail; a preview's output is never shown here.
    ...(failed ? { detail: detailFromToolPart(part) } : {}),
    ...(preview ? { preview } : {}),
  });
  if (kind === "agent") trackSubagentRow(live, callId, part);
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
  await live.client.replyPermission(id, toOpenCodePermissionReply(decision));
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
  showNextQuestion(live);
  if (reply.kind !== "answered") {
    await live.client.rejectQuestion(id);
    return;
  }
  const answers = questions.map((question) =>
    selectedAnswerLabels(question, reply),
  );
  await live.client.replyQuestion(id, answers);
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
  if (!next) return;
  const [requestId, { questions }] = next;
  live.onEvent({
    type: "question.asked",
    requestId,
    title: questionPromptTitle(questions) || "OpenCode question",
    questions,
  });
}

function finishActiveTurn(live: Live, extraEvents: HarnessEvent[] = []): void {
  if (live.prompt?.errorTimer) clearTimeout(live.prompt.errorTimer);
  live.activeTurn = false;
  for (const event of extraEvents) live.onEvent(event);
  const done = live.turnDone;
  live.turnDone = null;
  live.turnFailed = null;
  if (done) {
    done();
    return;
  }
}

function emitAssistantSnapshot(live: Live, part: OpenCodePart): void {
  if (part.type !== "text" && part.type !== "reasoning") return;
  live.onEvent({
    type: "message.part",
    partId: part.id,
    text: part.text ?? "",
    reasoning: part.type === "reasoning",
    streaming: typeof part.time?.end !== "number",
  });
}

async function reconcileIdlePrompt(live: Live): Promise<void> {
  const prompt = live.prompt;
  if (
    !prompt ||
    !live.activeTurn ||
    !prompt.accepted ||
    (!prompt.observed && !prompt.pendingError) ||
    live.cancelled ||
    live.muteUpdates
  )
    return;
  if (prompt.checking) return prompt.checking;
  prompt.idleSeen = false;
  prompt.checking = (async () => {
    if ((await live.client.sessionStatus(live.openCodeSessionId)) !== "idle")
      return;
    const messages = await live.client.getMessages(live.openCodeSessionId);
    if (live.prompt !== prompt || live.muteUpdates || live.cancelled) return;
    const relatedIDs = relatedPromptMessageIDs(messages, prompt.messageIDs);
    const relevant = messages.filter((message) => {
      const info = asRecord(message.info);
      return (
        stringField(info, "role") === "assistant" &&
        relatedIDs.has(stringField(info, "parentID") ?? "")
      );
    });
    const latest = relevant[relevant.length - 1];
    const info = asRecord(latest?.info);
    if (!info) {
      await reconcileBufferedError(live, prompt, false);
      return;
    }
    const error = info.error;
    const finish = stringField(info, "finish");
    if (
      !error &&
      (!finish ||
        finish === "tool-calls" ||
        stringField(info, "agent") === "compaction")
    ) {
      await reconcileBufferedError(
        live,
        prompt,
        !finish && !asRecord(info.time)?.completed,
      );
      return;
    }
    if (error)
      live.onEvent({
        type: "session.error",
        message: sessionErrorMessage(error),
      });
    finishActiveTurn(live, [
      { type: "message.completed" },
      { type: "reasoning.completed" },
    ]);
  })().finally(async () => {
    prompt.checking = null;
    if (prompt.idleSeen && live.prompt === prompt)
      await reconcileIdlePrompt(live);
  });
  return prompt.checking;
}

function scheduleBufferedErrorCheck(
  live: Live,
  prompt: ActivePrompt,
  delay: number,
): void {
  if (
    !prompt.accepted ||
    live.prompt !== prompt ||
    !live.activeTurn ||
    live.muteUpdates ||
    live.cancelled
  )
    return;
  if (prompt.errorTimer) clearTimeout(prompt.errorTimer);
  prompt.errorTimer = setTimeout(
    () => {
      prompt.errorTimer = undefined;
      void reconcileIdlePrompt(live).catch((error: unknown) => {
        if (
          live.prompt !== prompt ||
          !live.activeTurn ||
          live.muteUpdates ||
          live.cancelled
        )
          return;
        live.onEvent({
          type: "session.error",
          message: `Could not verify OpenCode error: ${error instanceof Error ? error.message : String(error)}`,
        });
        finishActiveTurn(live);
      });
    },
    Math.max(0, delay),
  );
}

async function reconcileBufferedError(
  live: Live,
  prompt: ActivePrompt,
  assistantRunning: boolean,
): Promise<void> {
  const pending = prompt.pendingError;
  if (!pending || assistantRunning) return;
  const remaining = pending.graceMs - (Date.now() - pending.progressAt);
  if (remaining > 0) {
    scheduleBufferedErrorCheck(live, prompt, remaining);
    return;
  }
  const progressAt = pending.progressAt;
  const ownedIDs = new Set(prompt.messageIDs);
  if (
    (await live.client.sessionStatus(live.openCodeSessionId)) !== "idle" ||
    live.prompt !== prompt ||
    !live.activeTurn ||
    live.muteUpdates ||
    live.cancelled ||
    prompt.pendingError !== pending ||
    pending.progressAt !== progressAt ||
    prompt.messageIDs.size !== ownedIDs.size ||
    [...ownedIDs].some((id) => !prompt.messageIDs.has(id))
  )
    return;
  live.onEvent({ type: "session.error", message: pending.message });
  finishActiveTurn(live, [
    { type: "message.completed" },
    { type: "reasoning.completed" },
  ]);
}

function relatedPromptMessageIDs(
  messages: OpenCodeMessage[],
  ownedIDs: Set<string>,
): Set<string> {
  const related = new Set<string>();
  const owned = messages.filter((message) =>
    ownedIDs.has(stringField(asRecord(message.info), "id") ?? ""),
  );
  if (owned.length !== ownedIDs.size) return related;
  const latestOwned = owned[owned.length - 1];
  const boundary = messages.indexOf(latestOwned);
  if (boundary < 0) return related;
  related.add(stringField(asRecord(latestOwned.info), "id")!);
  const created = asRecord(asRecord(messages[boundary].info)?.time)?.created;
  if (typeof created !== "number") return related;
  const ownedParts = replayContent(latestOwned.parts ?? []);
  let compacted = false;
  for (const message of messages.slice(boundary + 1)) {
    const info = asRecord(message.info);
    const id = stringField(info, "id");
    if (!id) continue;
    if (stringField(info, "role") === "assistant") {
      if (
        related.has(stringField(info, "parentID") ?? "") &&
        stringField(info, "agent") === "compaction" &&
        !info?.error &&
        stringField(info, "finish") === "stop"
      )
        compacted = true;
      continue;
    }
    if (stringField(info, "role") !== "user" || ownedIDs.has(id)) continue;
    const messageCreated = asRecord(info?.time)?.created;
    if (typeof messageCreated !== "number" || messageCreated < created)
      continue;
    const parts = (message.parts ?? [])
      .map(asRecord)
      .filter((part) => part !== null);
    const automaticCompaction =
      parts.length > 0 &&
      parts.every((part) => part.type === "compaction" && part.auto === true);
    const continuation =
      compacted &&
      parts.length > 0 &&
      parts.every(
        (part) =>
          part.type === "text" &&
          part.synthetic === true &&
          asRecord(part.metadata)?.compaction_continue === true,
      );
    const replay =
      compacted &&
      parts.length > 0 &&
      ownedParts === replayContent(message.parts ?? []);
    if (automaticCompaction || continuation || replay) related.add(id);
  }
  return related;
}

function replayContent(parts: unknown[]): string {
  return JSON.stringify(
    parts.flatMap((value) => {
      const part = asRecord(value);
      if (!part || part.type === "compaction") return [];
      if (part.type === "text")
        return [
          { type: "text", text: part.text, synthetic: part.synthetic === true },
        ];
      if (part.type === "file") {
        const mime = stringField(part, "mime") ?? "";
        if (mime.startsWith("image/") || mime === "application/pdf")
          return [
            {
              type: "text",
              text: `[Attached ${mime}: ${stringField(part, "filename") ?? "file"}]`,
              synthetic: false,
            },
          ];
        return [{ type: "file", mime, filename: part.filename, url: part.url }];
      }
      return [{ type: part.type }];
    }),
  );
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
    return undefined;
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

/**
 * A rejected native file remains in OpenCode's durable history and can make
 * every later prompt fail while converting that history for the provider.
 * Revert the original attachment turn before resuming; OpenCode removes the
 * reverted tail when the next prompt starts.
 */
async function repairUnsupportedFileTurn(
  client: OpenCodeClient,
  sessionID: string,
): Promise<void> {
  const messages = await client.getMessages(sessionID);
  if (!Array.isArray(messages)) return;
  const byId = new Map<string, OpenCodeMessage>();
  for (const message of messages) {
    const id = stringField(asRecord(message.info), "id");
    if (id) byId.set(id, message);
  }
  const failures = messages
    .map((message) => {
      const info = asRecord(message.info);
      if (stringField(info, "role") !== "assistant") return null;
      const mime = unsupportedFileMediaType(info?.error);
      const parentID = stringField(info, "parentID");
      if (!mime || !parentID) return null;
      const parent = byId.get(parentID);
      const hasRejectedFile = (parent?.parts ?? []).some((part) => {
        const record = asRecord(part);
        return (
          stringField(record, "type") === "file" &&
          stringField(record, "mime")?.toLowerCase() === mime
        );
      });
      if (!hasRejectedFile) return null;
      const time = asRecord(info?.time)?.created;
      if (typeof time !== "number") return null;
      return { messageID: parentID, created: time };
    })
    .filter(
      (failure): failure is { messageID: string; created: number } =>
        failure !== null,
    )
    .filter(({ created }) =>
      messages.every((message) => {
        const info = asRecord(message.info);
        if (stringField(info, "role") !== "assistant" || info?.error) {
          return true;
        }
        const time = asRecord(info?.time)?.created;
        return typeof time !== "number" || time <= created;
      }),
    )
    .sort((left, right) => left.created - right.created);
  const first = failures[0];
  if (first) await client.revertSession(sessionID, first.messageID);
}

function unsupportedFileMediaType(error: unknown): string | undefined {
  const message = sessionErrorMessage(error);
  if (!/functionality not supported/i.test(message)) return undefined;
  return message
    .match(/file part media type\s+([^\s'"`]+)/i)?.[1]
    ?.toLowerCase();
}

async function assertOpenCodeVersion(path: string, cwd: string): Promise<void> {
  const output = await execChild(path, ["--version"], cwd, "opencode").catch(
    () => "",
  );
  const version = parseOpenCodeVersion(output);
  if (!version || !isSupportedOpenCodeVersion(version))
    throw new Error(unsupportedOpenCodeVersionMessage(version ?? undefined));
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

/** Exported for tests. */
export function __openCodeTestReset(): void {
  liveByThread.clear();
  resumeByThread.clear();
  cancelledThreads.clear();
  openingThreads.clear();
  lifecycleByThread.clear();
}

async function withLifecycle<T>(
  sessionId: string,
  action: () => Promise<T>,
): Promise<T> {
  const previous = lifecycleByThread.get(sessionId) ?? Promise.resolve();
  let release!: () => void;
  const current = new Promise<void>((resolve) => {
    release = resolve;
  });
  lifecycleByThread.set(sessionId, current);
  await previous;
  try {
    return await action();
  } finally {
    release();
    if (lifecycleByThread.get(sessionId) === current)
      lifecycleByThread.delete(sessionId);
  }
}
