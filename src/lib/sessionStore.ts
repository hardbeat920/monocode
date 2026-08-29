import { invoke } from "@tauri-apps/api/core";
import { persistableAttachment } from "./attachments";
import type { ContextUsage } from "./contextUsage";
import { normalizeProjectPath } from "./recents";
import type {
  Block,
  HarnessId,
  HandoffMeta,
  HandoffStatus,
  RuntimeMode,
  SecondOpinionMeta,
  Session,
} from "./session";
import { HARNESSES, RUNTIME_MODES } from "./session";

export type SessionSummary = {
  id: string;
  cwd: string;
  harness: HarnessId;
  model: string;
  runtimeMode: RuntimeMode;
  title: string;
  providerSessionId?: string;
  branch?: string;
  repo?: string;
  additions?: number;
  deletions?: number;
  createdAt: number;
  updatedAt: number;
  archived?: boolean;
};

type SessionRecord = {
  id: string;
  cwd: string;
  harness: string;
  model: string;
  modelSettings: Record<string, string>;
  runtimeMode: string;
  title: string;
  providerSessionId?: string | null;
  blocks: Block[];
  contextUsed?: number | null;
  contextWindow?: number | null;
  branch?: string | null;
  worktreeCwd?: string | null;
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
  blocks: Block[];
  contextUsed?: number;
  contextWindow?: number;
  branch?: string;
  worktreeCwd?: string;
};

/** Only real chats belong in project history — blank tabs stay ephemeral. */
export function shouldPersistSession(session: Session): boolean {
  return (
    session.cwd !== "~" && session.blocks.some((block) => block.role === "user")
  );
}

/** Matches Rust `validate_id` — a path here fails the whole upsert. */
export function isPersistableId(value: string): boolean {
  return /^[A-Za-z0-9_-]+$/.test(value);
}

export function sanitizeSessionForPersist(
  session: Session,
): SessionUpsertPayload {
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
    blocks: session.blocks
      .map(sanitizeBlock)
      .filter((block): block is Block => block != null),
    ...(session.context ? { contextUsed: session.context.used } : {}),
    ...(session.context?.window
      ? { contextWindow: session.context.window }
      : {}),
    ...(session.branch ? { branch: session.branch } : {}),
    ...(session.worktreeCwd ? { worktreeCwd: session.worktreeCwd } : {}),
  };
}

/**
 * `session_upsert` runs off the main thread, so two writes for the same
 * session could otherwise land in either order and let an older transcript
 * overwrite a newer one. Chain them per session; different sessions still
 * write concurrently.
 */
const upsertQueues = new Map<string, Promise<unknown>>();

export async function upsertSession(
  session: Session,
): Promise<SessionSummary | null> {
  if (!shouldPersistSession(session)) return null;
  const payload = sanitizeSessionForPersist(session);
  const previous = upsertQueues.get(session.id) ?? Promise.resolve();
  const run = previous
    .catch(() => undefined)
    .then(() => invoke<SessionSummary>("session_upsert", { session: payload }));
  upsertQueues.set(session.id, run);
  try {
    return normalizeSummary(await run);
  } finally {
    if (upsertQueues.get(session.id) === run) upsertQueues.delete(session.id);
  }
}

export function persistFingerprint(session: Session): string {
  return JSON.stringify(sanitizeSessionForPersist(session));
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
  return recordToSession(record);
}

export async function deleteSession(sessionId: string): Promise<void> {
  await invoke<void>("session_delete", { sessionId });
}

export async function setSessionArchived(
  sessionId: string,
  archived: boolean,
): Promise<void> {
  await invoke<void>("session_set_archived", { sessionId, archived });
}

export async function replaceInFlightSessions(
  refs: { sessionId: string; cwd: string }[],
): Promise<void> {
  await invoke("session_set_in_flight", {
    sessions: refs.map((ref) => ({
      sessionId: ref.sessionId,
      cwd: normalizeProjectPath(ref.cwd),
    })),
  });
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

export async function saveWorkspaceSnapshot(snapshot: unknown): Promise<void> {
  await invoke("workspace_set_snapshot", { snapshot });
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
  if (block.startedAt != null) next.startedAt = block.startedAt;
  if (block.durationMs != null) next.durationMs = block.durationMs;
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
  const handoff = sanitizeHandoff(block.handoff);
  if (handoff) next.handoff = handoff;
  else if (block.role === "handoff") return null;
  const secondOpinion = sanitizeSecondOpinion(block.secondOpinion);
  if (secondOpinion) next.secondOpinion = secondOpinion;
  return next;
}

function normalizeSummary(summary: SessionSummary): SessionSummary {
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
    ...(summary.archived ? { archived: true } : {}),
  };
}

function recordToSession(record: SessionRecord): Session {
  const blocks = Array.isArray(record.blocks)
    ? record.blocks
        .map(sanitizeBlock)
        .filter((block): block is Block => block != null)
    : [];
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
    ...(record.providerSessionId
      ? { providerSessionId: record.providerSessionId }
      : {}),
    ...(record.branch ? { branch: record.branch } : {}),
    ...(record.worktreeCwd ? { worktreeCwd: record.worktreeCwd } : {}),
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
  };
}

function asRuntimeMode(value: string): RuntimeMode {
  return (RUNTIME_MODES as string[]).includes(value)
    ? (value as RuntimeMode)
    : "supervised";
}
