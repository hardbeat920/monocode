import { invoke } from "@tauri-apps/api/core";
import { recoverCursorSubagents } from "../../../integrations/harness/providers/cursor/cursorSubagents";
import { persistableAttachment } from "../model/attachments";
import type { ContextUsage } from "../model/contextUsage";
import { normalizeProjectPath } from "../../projects/model/recents";
import { ompActiveAssistantTexts, ompSessionInterjections } from "../../../platform/tauri/fs";
import { backfillOmpInterjections, ompStatusSplitTexts } from "../model/ompInterjections";
import type {
  AgentRunMeta,
  AgentStep,
  Block,
  GeneratedImageMeta,
  HarnessId,
  HandoffMeta,
  HandoffStatus,
  InterjectionMeta,
  LinkedWorkItem,
  RuntimeMode,
  SecondOpinionMeta,
  Session,
  TaskListMeta,
  PlanBlockMeta,
  TurnModel,
  TurnMetrics,
} from "../model/session";
import { HARNESSES, RUNTIME_MODES } from "../model/session";
import { restoreOrchestrationProposal } from "../../orchestration/model/orchestrationPlan";

import type { OrchestrationSummary } from "../../orchestration/model/orchestrationSummary";

export type SessionSummary = {
  orchestrationLeadId?: string;
  orchestration?: OrchestrationSummary;
  id: string;
  cwd: string;
  harness: HarnessId;
  model: string;
  runtimeMode: RuntimeMode;
  title: string;
  providerSessionId?: string;
  branch?: string;
  worktreeCwd?: string;
  worktreeRemoved?: boolean;
  repo?: string;
  additions?: number;
  deletions?: number;
  createdAt: number;
  updatedAt: number;
  archived?: boolean;
  pinned?: boolean;
  draft?: boolean;
  linkedWorkItem?: LinkedWorkItem;
  automationId?: string;
};

type SessionRecord = {
  orchestrationLeadId?: string;
  id: string;
  cwd: string;
  harness: string;
  model: string;
  modelSettings: Record<string, string>;
  runtimeMode: string;
  title: string;
  providerSessionId?: string | null;
  providerAccountId?: string | null;
  blocks: Block[];
  contextUsed?: number | null;
  contextWindow?: number | null;
  branch?: string | null;
  worktreeCwd?: string | null;
  worktreeRemoved?: boolean;
  linkedWorkItem?: LinkedWorkItem | null;
  automationId?: string | null;
  createdAt: number;
  updatedAt: number;
};

type SessionUpsertPayload = {
  id: string;
  cwd: string;
  harness: string;
  model: string;
  modelSettings: Record<string, string>;
  runtimeMode: string;
  title: string;
  providerSessionId?: string;
  providerAccountId?: string;
  blocks: Block[];
  contextUsed?: number;
  contextWindow?: number;
  branch?: string;
  worktreeCwd?: string;
  worktreeRemoved?: boolean;
  linkedWorkItem?: LinkedWorkItem;
  automationId?: string;
};

/** Only real chats belong in project history — blank tabs stay ephemeral. */
export function shouldPersistSession(session: Session): boolean {
  return (
    !session.inboxAsk &&
    session.cwd !== "~" &&
    session.blocks.some((block) => block.role === "user")
  );
}

/** Matches Rust `validate_id` — a path here fails the whole upsert. */
export function isPersistableId(value: string): boolean {
  return /^[A-Za-z0-9_-]+$/.test(value);
}

function persistableMeta(
  session: Session,
): Omit<SessionUpsertPayload, "blocks"> {
  const linkedWorkItem = sanitizeLinkedWorkItem(session.linkedWorkItem);
  return {
    id: session.id,
    cwd: normalizeProjectPath(session.cwd),
    harness: session.harness,
    model: session.model,
    modelSettings: session.modelSettings,
    runtimeMode: session.runtimeMode,
    title: session.title,
    ...(session.providerSessionId && isPersistableId(session.providerSessionId)
      ? { providerSessionId: session.providerSessionId }
      : {}),
    ...(session.providerAccountId && isPersistableId(session.providerAccountId)
      ? { providerAccountId: session.providerAccountId }
      : {}),
    ...(session.context ? { contextUsed: session.context.used } : {}),
    ...(session.context?.window
      ? { contextWindow: session.context.window }
      : {}),
    ...(session.branch ? { branch: session.branch } : {}),
    ...(session.worktreeCwd ? { worktreeCwd: session.worktreeCwd } : {}),
    ...(session.worktreeRemoved ? { worktreeRemoved: true } : {}),
    ...(linkedWorkItem ? { linkedWorkItem } : {}),
    ...(session.automationId && isPersistableId(session.automationId)
      ? { automationId: session.automationId }
      : {}),
  };
}

export function sanitizeLinkedWorkItem(
  value: unknown,
): LinkedWorkItem | undefined {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return undefined;
  }
  const item = value as Partial<LinkedWorkItem>;
  const kind = item.kind;
  const repo = typeof item.repo === "string" ? item.repo.trim() : "";
  const number = item.number;
  if (
    (kind !== "issue" && kind !== "pr") ||
    !/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(repo) ||
    typeof number !== "number" ||
    !Number.isSafeInteger(number) ||
    number <= 0
  ) {
    return undefined;
  }
  return {
    kind,
    repo,
    number,
    url: `https://github.com/${repo}/${kind === "pr" ? "pull" : "issues"}/${number}`,
  };
}

export function sanitizeSessionForPersist(
  session: Session,
): SessionUpsertPayload {
  const firstUser = session.blocks.findIndex((block) => block.role === "user");
  return {
    ...persistableMeta(session),
    blocks: session.blocks
      .map((block, index) =>
        sanitizeBlock(
          index === firstUser && session.orchestrationLeadId
            ? { ...block, orchestrationLeadId: session.orchestrationLeadId }
            : block,
        ),
      )
      .filter((block): block is Block => block != null),
  };
}

/**
 * `session_upsert` runs off the main thread, so two writes for the same
 * session could otherwise land in either order and let an older transcript
 * overwrite a newer one. Chain them per session; different sessions still
 * write concurrently.
 */
const sessionWriteQueues = new Map<string, Promise<unknown>>();
const sessionWriteLeadById = new Map<string, string>();
const deletedSessionIds = new Set<string>();

function enqueueSessionWrite<T>(
  sessionId: string,
  operation: () => Promise<T>,
): Promise<T> {
  const previous = sessionWriteQueues.get(sessionId) ?? Promise.resolve();
  const run = previous.catch(() => undefined).then(operation);
  const tail = run.then(
    () => undefined,
    () => undefined,
  );
  sessionWriteQueues.set(sessionId, tail);
  void tail.then(() => {
    if (sessionWriteQueues.get(sessionId) === tail) {
      sessionWriteQueues.delete(sessionId);
      sessionWriteLeadById.delete(sessionId);
    }
  });
  return run;
}

export async function upsertSession(
  session: Session,
): Promise<SessionSummary | null> {
  if (!shouldPersistSession(session) || deletedSessionIds.has(session.id)) {
    return null;
  }
  const payload = sanitizeSessionForPersist(session);
  if (session.orchestrationLeadId) {
    sessionWriteLeadById.set(session.id, session.orchestrationLeadId);
  } else {
    sessionWriteLeadById.delete(session.id);
  }
  const summary = await enqueueSessionWrite(session.id, async () => {
    if (deletedSessionIds.has(session.id)) return null;
    return invoke<SessionSummary>("session_upsert", {
      session: {
        ...payload,
        blocks: payload.blocks.map((block) =>
          block.orchestrationLeadId &&
          deletedSessionIds.has(block.orchestrationLeadId)
            ? { ...block, orchestrationLeadId: undefined }
            : block,
        ),
      },
    });
  });
  return summary ? normalizeSummary(summary) : null;
}

/**
 * Blocks are replaced, never mutated in place, so identity stands in for
 * content. Serializing the session here instead meant a full deep copy and a
 * `JSON.stringify` of the whole transcript — megabytes on a long chat — on the
 * main thread every time a save was considered. Header fields go through
 * `persistableMeta` so a new persisted column cannot be forgotten here.
 */
const blockTokens = new WeakMap<Block, number>();
let lastBlockToken = 0;

function blockToken(block: Block): number {
  const seen = blockTokens.get(block);
  if (seen !== undefined) return seen;
  const token = ++lastBlockToken;
  blockTokens.set(block, token);
  return token;
}

export function persistFingerprint(session: Session): string {
  return `${JSON.stringify(persistableMeta(session))}|${session.orchestrationLeadId ?? ""}|${session.blocks
    .map(blockToken)
    .join(",")}`;
}

export async function listSessionsByProject(
  cwd: string,
): Promise<SessionSummary[]> {
  if (!cwd || cwd === "~") return [];
  const rows = await invoke<SessionSummary[]>("session_list_by_project", {
    cwd: normalizeProjectPath(cwd),
  });
  return rows.map(normalizeSummary);
}

export function rebaseProjectSessions(
  fromCwd: string,
  toCwd: string,
): Promise<void> {
  return invoke<void>("session_rebase_project", {
    fromCwd: normalizeProjectPath(fromCwd),
    toCwd: normalizeProjectPath(toCwd),
  });
}

export async function listLinkedSessions(): Promise<SessionSummary[]> {
  const rows = await invoke<SessionSummary[]>("session_list_linked");
  return rows.map(normalizeSummary);
}

export type SessionSearchHit = {
  kind: "conversation" | "message";
  sessionId: string;
  cwd: string;
  harness: string;
  title: string;
  updatedAt: number;
  blockId?: string;
  role?: string;
  preview: string;
};

export type SessionSearchResult = {
  hits: SessionSearchHit[];
  truncated: boolean;
};

export async function searchSessions(options: {
  query: string;
  cwd?: string;
  includeArchived?: boolean;
}): Promise<SessionSearchResult> {
  const query = options.query.trim();
  if (!query) return { hits: [], truncated: false };
  const result = await invoke<SessionSearchResult>("session_search", {
    options: {
      query,
      ...(options.cwd && options.cwd !== "~"
        ? { cwd: normalizeProjectPath(options.cwd) }
        : {}),
      ...(options.includeArchived ? { includeArchived: true } : {}),
    },
  });
  return {
    hits: Array.isArray(result?.hits) ? result.hits : [],
    truncated: !!result?.truncated,
  };
}

export async function getSession(sessionId: string): Promise<Session | null> {
  const record = await invoke<SessionRecord | null>("session_get", {
    sessionId,
  });
  if (!record) return null;
  const session = recordToSession(record);
  if (session.harness !== "omp" || !session.providerSessionId) {
    return recoverCursorSubagents(session);
  }
  try {
    const anchors = await ompSessionInterjections(session.providerSessionId);
    // Missing source order must not prevent the existing anchored repair.
    const source = ompStatusSplitTexts(session.blocks).length
      ? await ompActiveAssistantTexts(session.providerSessionId).catch(() => [])
      : [];
    const blocks = backfillOmpInterjections(session.blocks, anchors, source);
    if (blocks !== session.blocks) {
      session.blocks = blocks;
      // Persist before exposing the restored session to a new live turn.
      // Re-reading the source on later loads allows partial repairs to retry;
      // deterministic IDs ensure already repaired transcripts are not written.
      await upsertSession(session);
    }
  } catch {
    // Source logs may be absent/unreadable. Even a failed write must not stop
    // restore; the recovered in-memory boundaries can still be displayed.
  }
  return session;
}

export async function deleteSession(
  sessionId: string,
  imagePaths: string[] = [],
): Promise<void> {
  deletedSessionIds.add(sessionId);
  try {
    // A lead with workers still has writes in flight. Finish those before
    // the deletion transaction strips their ownership metadata.
    const pendingWrites = [...sessionWriteQueues.entries()]
      .filter(
        ([queuedSessionId]) =>
          queuedSessionId === sessionId ||
          sessionWriteLeadById.get(queuedSessionId) === sessionId,
      )
      .map(([, pending]) => pending);
    if (pendingWrites.length > 0) await Promise.all(pendingWrites);
    await enqueueSessionWrite(sessionId, () =>
      invoke<void>("session_delete", { sessionId, imagePaths }),
    );
    const tombstone = setTimeout(() => deletedSessionIds.delete(sessionId), 60_000);
    if (typeof tombstone === "object") tombstone.unref();
  } catch (error) {
    deletedSessionIds.delete(sessionId);
    throw error;
  }
}

/** Delete a draft-only record while allowing its still-open blank session id to be saved later. */
export async function discardDraftSessionRecord(
  sessionId: string,
): Promise<void> {
  await enqueueSessionWrite(sessionId, () =>
    invoke<void>("session_delete", { sessionId, imagePaths: [] }),
  );
}

export async function setSessionArchived(
  sessionId: string,
  archived: boolean,
): Promise<void> {
  await enqueueSessionWrite(sessionId, () =>
    invoke<void>("session_set_archived", { sessionId, archived }),
  );
}

export async function setSessionPinned(
  sessionId: string,
  pinned: boolean,
): Promise<void> {
  await invoke<void>("session_set_pinned", { sessionId, pinned });
}

export async function setSessionLinkedWorkItem(
  sessionId: string,
  value: LinkedWorkItem | undefined,
): Promise<void> {
  const linkedWorkItem = sanitizeLinkedWorkItem(value);
  await enqueueSessionWrite(sessionId, () =>
    invoke<void>("session_set_linked_work_item", {
      sessionId,
      linkedWorkItem: linkedWorkItem ?? null,
    }),
  );
}

/** Drain pending saves before a worktree removal changes stored session context. */
export async function flushSessionWrites(): Promise<void> {
  await Promise.all([...sessionWriteQueues.values()]);
}

/**
 * `session_set_in_flight` runs off the main thread, so two replaces could
 * otherwise land in either order and restore a stale busy snapshot.
 */
let inFlightWrite: Promise<unknown> = Promise.resolve();

export async function replaceInFlightSessions(
  refs: { sessionId: string; cwd: string }[],
): Promise<void> {
  const run = inFlightWrite
    .catch(() => undefined)
    .then(() =>
      invoke("session_set_in_flight", {
        sessions: refs.map((ref) => ({
          sessionId: ref.sessionId,
          cwd: normalizeProjectPath(ref.cwd),
        })),
      }),
    );
  inFlightWrite = run;
  await run;
}

/** Kept across Vite reloads; boot must not delete the only copy. */
export async function listInFlightSessions(): Promise<
  { sessionId: string; cwd: string }[]
> {
  const rows = await invoke<{ sessionId: string; cwd: string }[]>(
    "session_list_in_flight",
  );
  return Array.isArray(rows) ? rows : [];
}

/** Destructive: the first window to boot after a quit owns these chats. */
export async function takeInFlightSessions(): Promise<
  { sessionId: string; cwd: string }[]
> {
  const rows = await invoke<{ sessionId: string; cwd: string }[]>(
    "session_take_in_flight",
  );
  return Array.isArray(rows) ? rows : [];
}

/**
 * `workspace_set_snapshot` runs off the main thread, so two saves could
 * otherwise finish out of order and keep an older layout.
 */
let workspaceWrite: Promise<unknown> = Promise.resolve();

export async function saveWorkspaceSnapshot(snapshot: unknown): Promise<void> {
  const run = workspaceWrite
    .catch(() => undefined)
    .then(() => invoke("workspace_set_snapshot", { snapshot }));
  workspaceWrite = run;
  await run;
}

export async function loadWorkspaceSnapshot(): Promise<unknown | null> {
  const raw = await invoke<unknown | null>("workspace_get_snapshot");
  return raw ?? null;
}

function sanitizeBlock(block: Block): Block | null {
  const next: Block = {
    id: block.id,
    role: block.role,
    text: block.text,
  };
  if (block.attachments?.length) {
    next.attachments = block.attachments.map(persistableAttachment);
  }
  const image = sanitizeGeneratedImage(block.image);
  if (block.role === "image" && !image) return null;
  if (image) next.image = image;
  if (block.startedAt != null) next.startedAt = block.startedAt;
  if (block.durationMs != null) next.durationMs = block.durationMs;
  const turnModel = sanitizeTurnModel(block.turnModel);
  if (block.role === "user" && turnModel) next.turnModel = turnModel;
  if (block.role === "user" && block.draft) next.draft = true;
  if (
    block.role === "user" &&
    typeof block.providerTurnId === "string" &&
    isPersistableId(block.providerTurnId)
  )
    next.providerTurnId = block.providerTurnId;
  if (
    block.role === "user" &&
    typeof block.orchestrationLeadId === "string" &&
    isPersistableId(block.orchestrationLeadId)
  )
    next.orchestrationLeadId = block.orchestrationLeadId;
  // Without this the transcript would show the app's orchestration turns as
  // the user's own after a reload.
  if (block.role === "user" && block.internal) next.internal = true;
  const turnMetrics = sanitizeTurnMetrics(block.turnMetrics);
  if (block.role === "user" && turnMetrics) next.turnMetrics = turnMetrics;
  if (block.tool) next.tool = block.tool;
  if (block.approval?.decided) {
    next.approval = {
      requestId: block.approval.requestId,
      decided: block.approval.decided,
    };
  } else if (block.approval && !block.approval.decided) {
    // Drop stale live approval prompts; request ids don't survive restarts.
    if (block.role === "approval") return null;
  }
  const agentRun = sanitizeAgentRun(block.agentRun);
  if (agentRun) next.agentRun = agentRun;
  const taskList = sanitizeTaskList(block.taskList);
  if (taskList) next.taskList = taskList;
  else if (block.role === "tasks") return null;
  const plan = sanitizePlan(block.plan, block.text);
  if (block.orchestration)
    next.orchestration = restoreOrchestrationProposal(block.orchestration);
  if (plan) next.plan = plan;
  else if (block.role === "plan") {
    next.plan = { status: "ready", originalText: block.text };
  }
  const handoff = sanitizeHandoff(block.handoff);
  if (handoff) next.handoff = handoff;
  else if (block.role === "handoff") return null;
  const secondOpinion = sanitizeSecondOpinion(block.secondOpinion);
  if (secondOpinion) next.secondOpinion = secondOpinion;
  const noteCard = sanitizeNoteCard(block.noteCard);
  if (noteCard) next.noteCard = noteCard;
  // Interjection chrome survives restarts only on system blocks; a malformed
  // payload keeps the ordinary system row rather than losing its body.
  if (block.role === "system") {
    const interjection = sanitizeInterjection(block.interjection);
    if (interjection) next.interjection = interjection;
    if (block.notice === "error" || block.notice === "interrupt") {
      next.notice = block.notice;
    }
  }
  return next;
}

function sanitizeGeneratedImage(value: unknown): GeneratedImageMeta | undefined {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return undefined;
  }
  const record = value as Record<string, unknown>;
  const path = typeof record.path === "string" ? record.path.trim() : "";
  const name = typeof record.name === "string" ? record.name.trim() : "";
  const mimeType = typeof record.mimeType === "string" ? record.mimeType.trim() : "";
  const size = record.size;
  if (
    !path ||
    !name ||
    !mimeType.startsWith("image/") ||
    typeof size !== "number" ||
    !Number.isSafeInteger(size) ||
    size <= 0
  ) {
    return undefined;
  }
  const alt = typeof record.alt === "string" ? record.alt.trim() : "";
  return {
    path,
    name,
    mimeType,
    size,
    ...(alt ? { alt } : {}),
  };
}

function sanitizeTurnMetrics(value: unknown): TurnMetrics | undefined {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return undefined;
  }
  const rec = value as Record<string, unknown>;
  const number = (key: keyof TurnMetrics): number | undefined => {
    const candidate = rec[key];
    return typeof candidate === "number" &&
      Number.isFinite(candidate) &&
      candidate >= 0
      ? candidate
      : undefined;
  };
  const metrics: TurnMetrics = {
    ...(number("inputTokens") != null
      ? { inputTokens: number("inputTokens") }
      : {}),
    ...(number("outputTokens") != null
      ? { outputTokens: number("outputTokens") }
      : {}),
    ...(number("cacheReadTokens") != null
      ? { cacheReadTokens: number("cacheReadTokens") }
      : {}),
    ...(number("cacheWriteTokens") != null
      ? { cacheWriteTokens: number("cacheWriteTokens") }
      : {}),
    ...(number("cacheHitPercent") != null
      ? { cacheHitPercent: number("cacheHitPercent") }
      : {}),
  };
  return Object.keys(metrics).length > 0 ? metrics : undefined;
}

function sanitizeTurnModel(value: unknown): TurnModel | undefined {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return undefined;
  }
  const record = value as Record<string, unknown>;
  const harness = record.harness;
  const id = typeof record.id === "string" ? record.id.trim() : "";
  const name = typeof record.name === "string" ? record.name.trim() : "";
  if (
    typeof harness !== "string" ||
    !HARNESSES.includes(harness as HarnessId) ||
    !id ||
    !name
  ) {
    return undefined;
  }
  return { harness: harness as HarnessId, id, name };
}

function sanitizeInterjection(
  value: Block["interjection"],
): InterjectionMeta | undefined {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return undefined;
  }
  const record = value as Record<string, unknown>;
  const customType =
    typeof record.customType === "string" ? record.customType.trim() : "";
  if (!customType) return undefined;
  const severity = record.severity;
  return {
    customType,
    ...(severity === "nit" || severity === "concern" || severity === "blocker"
      ? { severity }
      : {}),
  };
}

function sanitizePlan(value: unknown, text: string): PlanBlockMeta | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  const record = value as Record<string, unknown>;
  const status = record.status;
  if (
    status !== "streaming" &&
    status !== "ready" &&
    status !== "building" &&
    status !== "built"
  ) {
    return null;
  }
  const key = typeof record.key === "string" ? record.key.trim() : "";
  const originalText =
    typeof record.originalText === "string" ? record.originalText : text;
  const approvedText =
    typeof record.approvedText === "string" ? record.approvedText : "";
  return {
    ...(key ? { key } : {}),
    // A restarted app cannot still be executing this approval.
    status: status === "streaming" || status === "building" ? "ready" : status,
    ...(originalText ? { originalText } : {}),
    ...(approvedText ? { approvedText } : {}),
    ...(record.edited === true ? { edited: true } : {}),
  };
}

/**
 * How much of a delegated run's trail a saved session keeps. Reopening a
 * session is for reading what the subagent concluded, not for replaying every
 * call it made, and a long run would otherwise dominate the snapshot.
 */
const PERSISTED_AGENT_STEPS = 100;

function sanitizeAgentRun(value: unknown): AgentRunMeta | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  const record = value as Record<string, unknown>;
  if (!Array.isArray(record.steps)) return null;
  const steps = record.steps.flatMap((entry): AgentStep[] => {
    if (!entry || typeof entry !== "object" || Array.isArray(entry)) return [];
    const row = entry as Record<string, unknown>;
    const id = typeof row.id === "string" ? row.id : "";
    const kind = row.kind;
    if (
      !id ||
      (kind !== "tool" && kind !== "message" && kind !== "reasoning")
    ) {
      return [];
    }
    const text = typeof row.text === "string" ? row.text : "";
    return [
      {
        id,
        kind,
        text,
        ...(typeof row.toolKind === "string" ? { toolKind: row.toolKind } : {}),
        ...(typeof row.status === "string" ? { status: row.status } : {}),
        ...(row.preview && typeof row.preview === "object"
          ? { preview: row.preview as AgentStep["preview"] }
          : {}),
      },
    ];
  });
  const name = typeof record.name === "string" ? record.name.trim() : "";
  if (!name && steps.length === 0) return null;
  return {
    name: name || "Subagent",
    ...(typeof record.model === "string" && record.model.trim()
      ? { model: record.model.trim() }
      : {}),
    ...(typeof record.agentType === "string" && record.agentType.trim()
      ? { agentType: record.agentType.trim() }
      : {}),
    steps: steps.slice(-PERSISTED_AGENT_STEPS),
  };
}

function sanitizeTaskList(value: unknown): TaskListMeta | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  const record = value as Record<string, unknown>;
  if (!Array.isArray(record.items)) return null;
  const items = record.items.flatMap((item) => {
    if (!item || typeof item !== "object" || Array.isArray(item)) return [];
    const row = item as Record<string, unknown>;
    const text = typeof row.text === "string" ? row.text.trim() : "";
    const status = row.status;
    if (
      !text ||
      (status !== "pending" &&
        status !== "in_progress" &&
        status !== "completed" &&
        status !== "cancelled")
    ) {
      return [];
    }
    const id =
      typeof row.id === "string"
        ? row.id.trim()
        : typeof row.id === "number" && Number.isFinite(row.id)
          ? String(row.id)
          : "";
    return [
      { ...(id ? { id } : {}), text, status },
    ] satisfies TaskListMeta["items"];
  });
  if (items.length === 0) return null;
  const key = typeof record.key === "string" ? record.key.trim() : "";
  const explanation =
    typeof record.explanation === "string" ? record.explanation.trim() : "";
  return {
    ...(key ? { key } : {}),
    ...(explanation ? { explanation } : {}),
    items,
  };
}

function normalizeSummary(summary: SessionSummary): SessionSummary {
  const linkedWorkItem = sanitizeLinkedWorkItem(summary.linkedWorkItem);
  return {
    ...summary,
    harness: asHarness(summary.harness),
    runtimeMode: asRuntimeMode(summary.runtimeMode),
    ...(summary.providerSessionId
      ? { providerSessionId: summary.providerSessionId }
      : {}),
    ...(summary.branch ? { branch: summary.branch } : {}),
    ...(summary.repo ? { repo: summary.repo } : {}),
    additions: summary.additions ?? 0,
    deletions: summary.deletions ?? 0,
    archived: summary.archived || undefined,
    pinned: summary.pinned || undefined,
    draft: summary.draft || undefined,
    linkedWorkItem,
    ...(typeof summary.automationId === "string" &&
    isPersistableId(summary.automationId)
      ? { automationId: summary.automationId }
      : {}),
  };
}

function recordToSession(record: SessionRecord): Session {
  const blocks = Array.isArray(record.blocks)
    ? record.blocks
        .map(sanitizeBlock)
        .filter((block): block is Block => block != null)
    : [];
  const linkedWorkItem = sanitizeLinkedWorkItem(record.linkedWorkItem);
  return {
    id: record.id,
    cwd: record.cwd,
    harness: asHarness(record.harness),
    model: record.model,
    modelSettings:
      record.modelSettings && typeof record.modelSettings === "object"
        ? record.modelSettings
        : {},
    runtimeMode: asRuntimeMode(record.runtimeMode),
    title: record.title,
    blocks,
    busy: false,
    orchestrationLeadId:
      record.orchestrationLeadId ??
      blocks.find(
        (block) =>
          block.orchestrationLeadId && block.orchestrationLeadId !== record.id,
      )?.orchestrationLeadId,
    ...(record.providerSessionId
      ? { providerSessionId: record.providerSessionId }
      : {}),
    ...(record.providerAccountId
      ? { providerAccountId: record.providerAccountId }
      : {}),
    ...(record.branch ? { branch: record.branch } : {}),
    ...(record.worktreeCwd ? { worktreeCwd: record.worktreeCwd } : {}),
    ...(record.worktreeRemoved ? { worktreeRemoved: true } : {}),
    ...(linkedWorkItem ? { linkedWorkItem } : {}),
    ...(record.automationId && isPersistableId(record.automationId)
      ? { automationId: record.automationId }
      : {}),
    ...(contextFromRecord(record) ?? {}),
  };
}

/**
 * Last known reading from a stored session. The harness re-reports on the next
 * turn, so this only has to survive until then.
 */
function contextFromRecord(
  record: SessionRecord,
): { context: ContextUsage } | undefined {
  const used = record.contextUsed;
  if (typeof used !== "number" || !Number.isFinite(used) || used <= 0) {
    return undefined;
  }
  const window = record.contextWindow;
  return {
    context:
      typeof window === "number" && Number.isFinite(window) && window > 0
        ? { used, window }
        : { used },
  };
}

function asHarness(value: string): HarnessId {
  return (HARNESSES as string[]).includes(value)
    ? (value as HarnessId)
    : "cursor";
}

const HANDOFF_STATUSES: HandoffStatus[] = ["preparing", "ready"];

function sanitizeHandoff(value: Block["handoff"]): HandoffMeta | undefined {
  if (!value) return undefined;
  if (!(HARNESSES as string[]).includes(value.from)) return undefined;
  if (!(HARNESSES as string[]).includes(value.to)) return undefined;
  if (!HANDOFF_STATUSES.includes(value.status)) return undefined;
  const interrupted = value.status === "preparing";
  return {
    from: value.from,
    to: value.to,
    status: "ready",
    pending: interrupted || !!value.pending,
  };
}

function sanitizeSecondOpinion(
  value: Block["secondOpinion"],
): SecondOpinionMeta | undefined {
  if (!value) return undefined;
  if (!(HARNESSES as string[]).includes(value.from)) return undefined;
  if (!(HARNESSES as string[]).includes(value.to)) return undefined;
  const request =
    typeof value.request === "string" ? value.request.trim().slice(0, 240) : "";
  const files =
    typeof value.files === "number" && Number.isFinite(value.files)
      ? Math.max(0, Math.round(value.files))
      : 0;
  return {
    from: value.from,
    to: value.to,
    ...(request ? { request } : {}),
    ...(files > 0 ? { files } : {}),
    ...(value.kind === "handoff" ? { kind: "handoff" as const } : {}),
  };
}

function sanitizeNoteCard(value: Block["noteCard"]): Block["noteCard"] {
  if (!value || typeof value !== "object") return undefined;
  const id = typeof value.id === "string" ? value.id.trim() : "";
  if (!id) return undefined;
  const slug = typeof value.slug === "string" ? value.slug.trim() : "";
  const title = typeof value.title === "string" ? value.title.trim() : "";
  const sourceCwd =
    typeof value.sourceCwd === "string" ? value.sourceCwd.trim() : "";
  return {
    id,
    slug,
    title,
    ...(sourceCwd ? { sourceCwd } : {}),
  };
}

function asRuntimeMode(value: string): RuntimeMode {
  return (RUNTIME_MODES as string[]).includes(value)
    ? (value as RuntimeMode)
    : "supervised";
}
