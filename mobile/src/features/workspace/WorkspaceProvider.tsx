import {
  closeWatch,
  hasNativeTranscript,
  makeCommandId,
  nextUpdates,
  openWatch,
} from "@/features/connection/nativeLink";
import {
  createContext,
  use,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { AppState } from "react-native";
import Storage from "expo-sqlite/kv-store";
import {
  useConnection,
  type Conversation,
} from "@/features/connection/ConnectionProvider";
import { selectedWorkspace } from "@/features/connection/selection";
import type { ModelCatalog, ModelChoice } from "@/features/models/types";
import { toWire, type Attachment } from "@/features/sessions/attachments";
import type { Message, Mono, Project, SessionSummary, Turn, TurnItem } from "./types";
import { applyFrames, type WatchFrame } from "./watchUpdates";
/**
 * The workspace: what the desktop has and what can be done with it. Changes
 * with the catalog, not with every streamed token, so the drawer and screens
 * reading it stay still while a reply streams.
 */
/** A new session's model and permissions, and where it works if not the
 *  project's checkout: a new worktree off HEAD, or an existing one. */
type StartChoice = Partial<ModelChoice> & { workspaceMode?: "worktree"; worktreePath?: string };

type Workspace = {
  projects: Project[];
  monos: Mono[];
  sessions: SessionSummary[];
  watch: (id: string | null) => void;
  startSession: (
    projectId: string,
    text: string,
    choice?: StartChoice,
    attachments?: Attachment[],
  ) => Promise<string>;
  sendMessage: (conversationId: string, text: string, attachments?: Attachment[]) => Promise<void>;
  stop: (conversationId: string) => Promise<void>;
  /** A page of a project's sessions, newest first, beyond the recent ones the poll carries. */
  listSessions: (
    projectId: string,
    query: string,
    offset: number,
    filters?: SessionListFilters,
  ) => Promise<SessionPage>;
  /** Keeps a paged-in session known, so its screen has its title and model. */
  rememberSession: (session: SessionSummary) => void;
  loadOlder: (conversationId: string) => Promise<void>;
  /** The model sheet's catalog, for a conversation or else a new session in the project. */
  listModels: (projectId: string, conversationId?: string) => Promise<ModelCatalog>;
  /** Sets a conversation's model and settings, or its permissions, as the desktop's pickers do. */
  configureSession: (conversationId: string, choice: Partial<ModelChoice>) => Promise<void>;
  /** Renames a session as the desktop sidebar does; the new title shows at once. */
  renameSession: (session: SessionSummary, title: string) => Promise<void>;
  /** Deletes a session on the desktop. It leaves every list at once, and comes
   *  back if the desktop refuses. */
  deleteSession: (conversationId: string) => Promise<void>;
  /** Archives a session on the desktop; like a delete, it leaves every list at once. */
  archiveSession: (conversationId: string) => Promise<void>;
  /** A session as the phone last changed it, or null once deleted here. */
  edited: (session: SessionSummary) => SessionSummary | null;
  nativeTranscriptStatus: (id: string, status: NativeTranscriptStatus) => void;
};
/** The open conversations' turns as they stream in; changes with every frame. */
type Live = {
  messages: Record<string, Message[]>;
  turns: Record<string, Turn[]>;
  pending: Record<string, PendingSend>;
  /** Turns paged in before the ones the watch carries. */
  history: Record<string, History>;
  /** Whether the watch's window starts after the conversation's first turn. */
  windowOlder: Record<string, boolean>;
};
/** Whether each conversation works, and why one could not be read. Changes
 *  far less often than the turns, so the screen around a transcript reads
 *  this and leaves the streaming to the transcript. */
type Status = {
  busy: Record<string, boolean>;
  errors: Record<string, string>;
};
export type SessionPage = { sessions: SessionSummary[]; hasMore: boolean };
/** Narrows a page to sessions active since a time, and without some providers.
 *  An older desktop ignores them. */
export type SessionListFilters = { since: number; hiddenHarnesses: string[] };
export const SESSION_PAGE = 30;
/** Earlier turns of a conversation, and whether there are earlier still. */
type History = { turns: Turn[]; older: boolean };
/** Turns a conversation opens on; older ones page in as the reader scrolls up. */
const TAIL = 3;
const OLDER_PAGE = 20;
/** An accepted new session may precede its desktop routing catalog by one poll. */
const STARTUP_GRACE_MS = 5_000;

/** A message sent from here that the desktop has not echoed back yet. */
type PendingSend = { turn: Turn; userCount: number; afterUserId?: string };
export type NativeTranscriptStatus = {
  busy: boolean;
  error: string | null;
  hasTurns: boolean;
  userCount: number;
  pendingId: string | null;
  ready?: boolean;
  lastUserId?: string | null;
  notice?: Extract<TurnItem, { type: "notice" }> | null;
};
const WorkspaceContext = createContext<Workspace | null>(null);
const LiveContext = createContext<Live | null>(null);
const StatusContext = createContext<Status | null>(null);

/** Conversations whose latest turns are kept on the phone, so one opened
 *  again after a restart shows at once while its watch reopens. */
const CACHED_CONVERSATIONS = 12;
const cacheKey = (desktop: string, id: string) =>
  `monocode.mobile.${desktop}.turns.${id}`;
const cacheIndexKey = (desktop: string) => `monocode.mobile.${desktop}.turns`;

async function readCachedTurns(desktop: string, id: string): Promise<Turn[] | null> {
  try {
    const saved = await Storage.getItem(cacheKey(desktop, id));
    const turns = saved ? JSON.parse(saved) : null;
    return Array.isArray(turns) ? turns : null;
  } catch {
    return null;
  }
}

/** Keeps a conversation's latest turns, the most recent conversations only. */
async function writeCachedTurns(desktop: string, id: string, turns: Turn[]) {
  // Its own local echo is not the desktop's.
  const kept = turns.filter((turn) => !turn.id.startsWith("pending:")).slice(-TAIL);
  if (kept.length === 0) return;
  try {
    const saved = await Storage.getItem(cacheIndexKey(desktop));
    const index: string[] = saved ? JSON.parse(saved) : [];
    const next = [id, ...index.filter((other) => other !== id)];
    await Promise.all([
      Storage.setItem(cacheKey(desktop, id), JSON.stringify(kept)),
      Storage.setItem(cacheIndexKey(desktop), JSON.stringify(next.slice(0, CACHED_CONVERSATIONS))),
      ...next.slice(CACHED_CONVERSATIONS).map((old) => Storage.removeItem(cacheKey(desktop, old))),
    ]);
  } catch {
    // A cache only; the watch brings the turns either way.
  }
}
export function WorkspaceProvider({ children }: { children: ReactNode }) {
  const connection = useConnection();
  const current = useRef(connection);
  useLayoutEffect(() => {
    current.current = connection;
  }, [connection]);
  const [messages, setMessages] = useState<Record<string, Message[]>>({});
  const [turns, setTurns] = useState<Record<string, Turn[]>>({});
  const turnsRef = useRef(turns);
  useLayoutEffect(() => {
    turnsRef.current = turns;
  }, [turns]);
  const [busy, setBusy] = useState<Record<string, boolean>>({});
  const [pending, setPending] = useState<Record<string, PendingSend>>({});
  const nativeUserCounts = useRef<Record<string, number>>({});
  const nativeLastUsers = useRef<Record<string, string | null>>({});
  const starting = useRef(new Map<string, number>());
  const [history, setHistory] = useState<Record<string, History>>({});
  const historyRef = useRef(history);
  useLayoutEffect(() => {
    historyRef.current = history;
  }, [history]);
  const [windowOlder, setWindowOlder] = useState<Record<string, boolean>>({});
  const [remembered, setRemembered] = useState<Record<string, SessionSummary>>({});
  const [errors, setErrors] = useState<Record<string, string>>({});
  // Renames and deletes made here, until the desktop's catalog catches up. A
  // rename holds only while the desktop still has the title it replaced.
  const [renamed, setRenamed] = useState<Record<string, { title: string; from: string }>>({});
  const [deleted, setDeleted] = useState<ReadonlySet<string>>(new Set());
  const [watching, watch] = useState<string | null>(null);
  // Conversations showing turns from the disk cache, until the desktop's arrive.
  // They may be long out of date, so a watch opens on the latest turns rather
  // than carrying on from them.
  const cachedOnly = useRef(new Set<string>());
  const desktop = connection.pairing?.desktop.fingerprint;
  const read = useCallback(async (id: string) => {
    const paired = current.current.pairing;
    try {
      const result = await current.current.command<Conversation>(
        "conversation.read",
        { conversationId: id },
      );
      if (current.current.pairing !== paired) return;
      cachedOnly.current.delete(id);
      const received = result.turns ?? turnsFromMessages(result.messages);
      setMessages((previous) => ({ ...previous, [id]: result.messages }));
      setTurns((previous) => ({
        ...previous,
        [id]: keepUnchanged(previous[id], received),
      }));
      setPending((previous) => withoutConfirmedPending(previous, id, received));
      if (countUserItems(received) > 0) starting.current.delete(id);
      setBusy((previous) => withValue(previous, id, result.busy));
      // A read carries the whole conversation.
      setWindowOlder((previous) => withValue(previous, id, false));
      setErrors((previous) => withoutKey(previous, id));
    } catch (reason) {
      const message = reason instanceof Error ? reason.message : String(reason);
      if (current.current.pairing === paired && !startupCatalogError(starting.current, id, message))
        setErrors((previous) => withValue(previous, id, message));
    }
  }, []);
  // The open conversation streams from the desktop: one watch, whose frames
  // land as the desktop sends them. A desktop that cannot watch, or a dropped
  // link, falls back to reading once a second until a watch opens again.
  useEffect(() => {
    if (!watching || !desktop || hasNativeTranscript) return;
    const id = watching;
    let disposed = false;
    let watchId: number | null = null;
    const pause = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
    // What the watch has built so far: frames are changes against it.
    let known: Turn[] = [];
    // The turn a reopened watch was asked to carry on from. If it opens
    // elsewhere, the history paged in before may no longer join up with it.
    let from: string | null = null;
    // Timing for the first frame, to see where opening a conversation spends.
    const opened = Date.now();
    let firstFrame = true;
    const apply = (frames: WatchFrame[]) => {
      const applied = applyFrames(known, frames);
      if (!applied) return false;
      if (__DEV__ && firstFrame) {
        firstFrame = false;
        console.log(`[mobile] ${id}: first frame after ${Date.now() - opened}ms`);
      }
      if (from && applied.turns.length > 0) {
        if (applied.turns[0].id !== from)
          setHistory((previous) => withoutKey(previous, id));
        from = null;
      }
      cachedOnly.current.delete(id);
      if (applied.turns !== known) {
        known = applied.turns;
        setTurns((previous) => ({ ...previous, [id]: applied.turns }));
      }
      setPending((previous) => withoutConfirmedPending(previous, id, applied.turns));
      if (countUserItems(applied.turns) > 0) starting.current.delete(id);
      if (applied.busy !== undefined) setBusy((previous) => withValue(previous, id, applied.busy!));
      if (applied.older !== undefined)
        setWindowOlder((previous) => withValue(previous, id, applied.older!));
      const error = applied.error && !startupCatalogError(starting.current, id, applied.error)
        ? applied.error
        : undefined;
      setErrors((previous) =>
        error ? withValue(previous, id, error) : withoutKey(previous, id),
      );
      return true;
    };
    const run = async () => {
      // Opened before, on an earlier run of the app: show its last turns
      // while the watch opens.
      if (!turnsRef.current[id]) {
        const cached = await readCachedTurns(desktop, id);
        if (disposed) return;
        if (cached && !turnsRef.current[id]) {
          cachedOnly.current.add(id);
          turnsRef.current = { ...turnsRef.current, [id]: cached };
          setTurns((previous) => (previous[id] ? previous : { ...previous, [id]: cached }));
        }
      }
      while (!disposed) {
        const pairing = current.current.pairing;
        if (AppState.currentState !== "active" || !pairing) {
          await pause(500);
          continue;
        }
        try {
          // Open on the latest turns, or on from the first one already here so
          // the history paged in before still joins up.
          known = turnsRef.current[id] ?? [];
          // Carry on from the first turn already here, so a conversation opened
          // before comes back as it was rather than cut to the latest turns.
          from = cachedOnly.current.has(id) ? null : (known[0]?.id ?? null);
          watchId = await openWatch(pairing, "conversation.watch", {
            conversationId: id,
            tail: TAIL,
            ...(from ? { from } : {}),
          });
          if (disposed) break;
          if (__DEV__) console.log(`[mobile] ${id}: watch open after ${Date.now() - opened}ms`);
          // The first frame brings every turn; keep the ones that did not change.
          known = turnsRef.current[id] ?? [];
          while (!disposed && AppState.currentState === "active") {
            const frames = await nextUpdates<WatchFrame>(watchId, 10_000);
            if (disposed || current.current.pairing !== pairing) break;
            if (frames.length && !apply(frames)) break;
          }
        } catch {
          if (disposed) break;
          await read(id);
          await pause((starting.current.get(id) ?? 0) > Date.now() ? 200 : 1000);
        }
        if (watchId !== null) void closeWatch(watchId);
        watchId = null;
      }
    };
    void run();
    // Kept for next time once the reader leaves it, or the app goes to the background.
    const save = () => {
      const turns = turnsRef.current[id];
      if (turns) void writeCachedTurns(desktop, id, turns);
    };
    const subscription = AppState.addEventListener("change", (state) => {
      if (state === "background") save();
    });
    return () => {
      disposed = true;
      subscription.remove();
      save();
      if (watchId !== null) void closeWatch(watchId);
    };
  }, [watching, desktop, read]);
  const mutationIds = useRef(new Map<string, string>());
  const { command, refresh } = connection;
  const mutate = useCallback(async <T,>(method: string, params: object): Promise<T> => {
    const signature = JSON.stringify([method, params]);
    const pending = mutationIds.current;
    let id = pending.get(signature);
    if (!id) {
      id = makeCommandId();
      pending.set(signature, id);
    }
    try {
      const result = await command<T>(method, params, id);
      pending.delete(signature);
      return result;
    } catch (reason) {
      // Keep the ID for a user retry after a lost reply. The desktop deduplicates it.
      if (pending.size > 128) {
        const oldest = pending.keys().next().value;
        if (oldest) pending.delete(oldest);
      }
      throw reason;
    }
  }, [command]);
  const startSession = useCallback(
    async (projectId: string, text: string, choice?: StartChoice, attachments: Attachment[] = []) => {
      const at = Date.now();
      const result = await mutate<{ sessionId: string }>("session.start", {
        projectId,
        text,
        ...(attachments.length ? { attachments: toWire(attachments) } : {}),
        ...choice,
      });
      // The new route can show its first prompt immediately, before a watch's
      // first frame arrives. Keep it separate from turns so its local ID is
      // never used as a desktop watch cursor, and retire it only on confirmation.
      const id = `pending:${at}`;
      starting.current.set(result.sessionId, Date.now() + STARTUP_GRACE_MS);
      setErrors((previous) => withoutKey(previous, result.sessionId));
      setPending((previous) => ({
        ...previous,
        [result.sessionId]: {
          turn: { id, items: [userItem(id, text, at, attachments)] },
          userCount: 0,
        },
      }));
      setBusy((previous) => withValue(previous, result.sessionId, true));
      // Catalog refresh updates the sidebar; it must not delay opening the chat.
      void refresh();
      return result.sessionId;
    },
    [mutate, refresh],
  );
  // The message shows at once, as its own turn, until the desktop's transcript
  // has one more user message than when it was sent.
  const sendMessage = useCallback(async (conversationId: string, text: string, attachments: Attachment[] = []) => {
    const at = Date.now();
    const id = `pending:${at}`;
    const userCount = hasNativeTranscript
      ? (nativeUserCounts.current[conversationId] ?? 0)
      : countUserItems(turnsRef.current[conversationId]);
    setPending((previous) => ({
      ...previous,
      [conversationId]: {
        turn: { id, items: [userItem(id, text, at, attachments)] },
        userCount,
        ...(hasNativeTranscript && nativeLastUsers.current[conversationId]
          ? { afterUserId: nativeLastUsers.current[conversationId]! } : {}),
      },
    }));
    try {
      await mutate("conversation.send", {
        conversationId,
        text,
        ...(attachments.length ? { attachments: toWire(attachments) } : {}),
      });
    } catch (reason) {
      setPending((previous) => withoutKey(previous, conversationId));
      throw reason;
    }
    if (hasNativeTranscript) return; // The native watch retires the local echo.
    await read(conversationId);
    // The read has the message; the watch may have brought it already.
    setPending((previous) =>
      previous[conversationId]?.turn.id === id ? withoutKey(previous, conversationId) : previous,
    );
  }, [mutate, read]);
  // Pages in the turns before the earliest one here.
  const loadingOlder = useRef(new Set<string>());
  const loadOlder = useCallback(
    async (conversationId: string) => {
      if (loadingOlder.current.has(conversationId)) return;
      const before = (
        historyRef.current[conversationId]?.turns[0] ?? turnsRef.current[conversationId]?.[0]
      )?.id;
      if (!before) return;
      loadingOlder.current.add(conversationId);
      try {
        const page = await command<History>("conversation.older", {
          conversationId,
          before,
          limit: OLDER_PAGE,
        });
        setHistory((previous) => ({
          ...previous,
          [conversationId]: {
            turns: [...page.turns, ...(previous[conversationId]?.turns ?? [])],
            older: page.older,
          },
        }));
      } finally {
        loadingOlder.current.delete(conversationId);
      }
    },
    [command],
  );
  const listModels = useCallback(
    (projectId: string, conversationId?: string) =>
      command<ModelCatalog>("models.list", {
        projectId,
        ...(conversationId ? { conversationId } : {}),
      }),
    [command],
  );
  const configureSession = useCallback(
    async (conversationId: string, choice: Partial<ModelChoice>) => {
      await mutate("session.configure", { conversationId, ...choice });
    },
    [mutate],
  );
  const stop = useCallback(
    async (conversationId: string) => {
      await mutate("session.cancel", { conversationId });
      if (!hasNativeTranscript) await read(conversationId);
    },
    [mutate, read],
  );
  const renameSession = useCallback(
    async (session: SessionSummary, title: string) => {
      await mutate("session.rename", { conversationId: session.id, title });
      setRenamed((previous) => ({ ...previous, [session.id]: { title, from: session.title } }));
      void refresh();
    },
    [mutate, refresh],
  );
  const removeSession = useCallback(
    async (method: "session.delete" | "session.archive", conversationId: string) => {
      setDeleted((previous) => new Set(previous).add(conversationId));
      try {
        await mutate(method, { conversationId });
      } catch (reason) {
        setDeleted((previous) => {
          const next = new Set(previous);
          next.delete(conversationId);
          return next;
        });
        throw reason;
      }
      void refresh();
    },
    [mutate, refresh],
  );
  const deleteSession = useCallback(
    (conversationId: string) => removeSession("session.delete", conversationId),
    [removeSession],
  );
  const archiveSession = useCallback(
    (conversationId: string) => removeSession("session.archive", conversationId),
    [removeSession],
  );
  const listSessions = useCallback(
    (projectId: string, query: string, offset: number, filters?: SessionListFilters) =>
      command<SessionPage>("sessions.list", {
        projectId,
        query,
        offset,
        limit: SESSION_PAGE,
        ...filters,
      }),
    [command],
  );
  const { catalog, selection } = connection;
  // The sessions the workspace already lists, read as a session is opened.
  const listedRef = useRef<ReadonlySet<string>>(new Set());
  useLayoutEffect(() => {
    listedRef.current = new Set([
      ...selectedWorkspace(catalog, selection).sessions.map((session) => session.id),
      ...Object.keys(remembered),
    ]);
  }, [catalog, selection, remembered]);
  // Only a session the workspace doesn't list yet (one paged in) changes it:
  // opening a listed one must not re-render every screen as it is pushed.
  const rememberSession = useCallback((session: SessionSummary) => {
    if (listedRef.current.has(session.id)) return;
    setRemembered((previous) => ({ ...previous, [session.id]: session }));
  }, []);
  const nativeTranscriptStatus = useCallback((id: string, status: NativeTranscriptStatus) => {
    nativeUserCounts.current[id] = status.userCount;
    nativeLastUsers.current[id] = status.lastUserId ?? null;
    if (status.ready === false && !status.error) return;
    const error = status.error && !startupCatalogError(starting.current, id, status.error)
      ? status.error : null;
    setErrors((previous) => error ? withValue(previous, id, error) : withoutKey(previous, id));
    if (status.ready === false) return;
    setBusy((previous) => withValue(previous, id, status.busy));
    if (status.userCount > 0) starting.current.delete(id);
    if (!status.pendingId) {
      setPending((previous) => {
        const sent = previous[id];
        const confirmed = sent?.afterUserId
          ? !!status.lastUserId && status.lastUserId !== sent.afterUserId
          : !!sent && status.userCount > sent.userCount;
        return confirmed ? withoutKey(previous, id) : previous;
      });
    }
  }, []);
  const workspace = useMemo<Workspace>(() => {
    const selected = selectedWorkspace(catalog, selection);
    const known = new Set(selected.sessions.map((session) => session.id));
    const edited = (session: SessionSummary) =>
      deleted.has(session.id)
        ? null
        : renamed[session.id]?.from === session.title
          ? { ...session, title: renamed[session.id].title }
          : session;
    return {
      projects: selected.projects,
      monos: selected.monos,
      sessions: [
        ...selected.sessions,
        ...Object.values(remembered).filter((session) => !known.has(session.id)),
      ].flatMap((session) => edited(session) ?? []),
      watch,
      startSession,
      sendMessage,
      stop,
      listSessions,
      rememberSession,
      loadOlder,
      listModels,
      configureSession,
      renameSession,
      deleteSession,
      archiveSession,
      edited,
      nativeTranscriptStatus,
    };
  }, [
    renamed,
    deleted,
    catalog,
    selection,
    remembered,
    startSession,
    sendMessage,
    stop,
    listSessions,
    rememberSession,
    loadOlder,
    listModels,
    configureSession,
    renameSession,
    deleteSession,
    archiveSession,
    nativeTranscriptStatus,
  ]);
  const live = useMemo<Live>(
    () => ({ messages, turns, pending, history, windowOlder }),
    [messages, turns, pending, history, windowOlder],
  );
  const status = useMemo<Status>(() => ({ busy, errors }), [busy, errors]);
  return (
    <WorkspaceContext value={workspace}>
      <StatusContext value={status}>
        <LiveContext value={live}>{children}</LiveContext>
      </StatusContext>
    </WorkspaceContext>
  );
}
export function useWorkspace() {
  const workspace = use(WorkspaceContext);
  if (!workspace)
    throw new Error("useWorkspace must be used inside WorkspaceProvider");
  return workspace;
}
/** The open conversations as they stream in. Re-renders with every frame. */
function useLive() {
  const live = use(LiveContext);
  if (!live) throw new Error("useLive must be used inside WorkspaceProvider");
  return live;
}
/**
 * Watches a conversation while mounted, and says whether it works and why it
 * could not be read. Re-renders only when those change, not as turns stream
 * in: read the turns with useConversationTurns, in the transcript itself.
 * The watch opens only once `watching`, say once a pushed screen is moving.
 */
export function useConversation(id: string | null, watching = true) {
  const { watch } = useWorkspace();
  const status = use(StatusContext);
  if (!status) throw new Error("useConversation must be used inside WorkspaceProvider");
  useEffect(() => {
    if (!watching) return;
    watch(id);
    return () => watch(null);
  }, [id, watch, watching]);
  return {
    busy: id ? !!status.busy[id] : false,
    error: id ? (status.errors[id] ?? null) : null,
  };
}
/** A conversation's turns as they stream in. Re-renders with every frame. */
export function useConversationTurns(id: string | null) {
  const { loadOlder } = useWorkspace();
  const { messages, turns, pending, history, windowOlder } = useLive();
  const current = id ? turns[id] : undefined;
  const earlier = id ? history[id] : undefined;
  const sent = id ? pending[id] : undefined;
  // Stable while nothing changes, so the list's rows keep their identity.
  const merged = useMemo(
    () => withPending(withHistory(earlier?.turns, current ?? []), sent),
    [earlier, current, sent],
  );
  const hasOlder = id ? (earlier ? earlier.older : !!windowOlder[id]) : false;
  return {
    messages: id ? (messages[id] ?? []) : [],
    turns: id ? merged : [],
    hasOlder,
    loadOlder: useCallback(() => (id ? loadOlder(id) : Promise.resolve()), [id, loadOlder]),
  };
}
/** Local sends only. Streamed turns are owned by the native view. */
export function useNativeTranscript(id: string) {
  const { pending } = useLive();
  const { nativeTranscriptStatus } = useWorkspace();
  return {
    pending: pending[id] ?? null,
    onStatus: useCallback((status: NativeTranscriptStatus) => nativeTranscriptStatus(id, status), [id, nativeTranscriptStatus]),
  };
}
export function monoConversationId(monoId: string) {
  return "mono:" + monoId;
}

function countUserItems(turns: Turn[] | undefined) {
  let count = 0;
  for (const turn of turns ?? []) {
    for (const item of turn.items) if (item.type === "user") count++;
  }
  return count;
}

/** Paged-in history ahead of the watched turns, without any turn twice. */
function withHistory(earlier: Turn[] | undefined, turns: Turn[]): Turn[] {
  if (!earlier?.length) return turns;
  const watched = new Set(turns.map((turn) => turn.id));
  return [...earlier.filter((turn) => !watched.has(turn.id)), ...turns];
}

function withPending(turns: Turn[], send: PendingSend | undefined): Turn[] {
  return send && countUserItems(turns) <= send.userCount ? [...turns, send.turn] : turns;
}

function withoutConfirmedPending(
  pending: Record<string, PendingSend>,
  id: string,
  turns: Turn[],
): Record<string, PendingSend> {
  const sent = pending[id];
  return sent && countUserItems(turns) > sent.userCount ? withoutKey(pending, id) : pending;
}

function startupCatalogError(starting: Map<string, number>, id: string, message: string) {
  return (starting.get(id) ?? 0) > Date.now() &&
    message === "This workspace item is no longer available";
}

/** The record with `key` set, or the same record when it already was. */
/** The message as shown until the desktop's transcript has it. */
function userItem(id: string, text: string, at: number, attachments: Attachment[]): TurnItem {
  return {
    type: "user",
    id,
    text,
    at,
    ...(attachments.length ? { attachments: attachments.length } : {}),
  };
}

function withValue<T>(record: Record<string, T>, key: string, value: T): Record<string, T> {
  return record[key] === value ? record : { ...record, [key]: value };
}

function withoutKey<T>(record: Record<string, T>, key: string): Record<string, T> {
  if (!(key in record)) return record;
  const { [key]: _, ...rest } = record;
  return rest;
}

/**
 * The poll sends the whole transcript every second. Keeping the turns that did
 * not change lets memoized rows skip re-rendering their markdown.
 */
function keepUnchanged(previous: Turn[] | undefined, next: Turn[]): Turn[] {
  if (!previous) return next;
  const known = new Map(previous.map((turn) => [turn.id, turn]));
  const merged = next.map((turn) => {
    const before = known.get(turn.id);
    return before && JSON.stringify(before) === JSON.stringify(turn) ? before : turn;
  });
  return merged.length === previous.length && merged.every((turn, i) => turn === previous[i])
    ? previous
    : merged;
}

/** Plain turns for a desktop that only sends messages. */
function turnsFromMessages(messages: Message[]): Turn[] {
  const turns: Turn[] = [];
  for (const message of messages) {
    const item =
      message.role === "user"
        ? { type: "user" as const, id: message.id, text: message.text }
        : { type: "text" as const, id: message.id, text: message.text };
    const current = turns.at(-1);
    if (!current || message.role === "user") turns.push({ id: message.id, items: [item] });
    else current.items.push(item);
  }
  return turns;
}
