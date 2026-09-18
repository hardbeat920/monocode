import { invoke } from "@tauri-apps/api/core";
import { ask } from "@tauri-apps/plugin-dialog";
import {
  bindHarnessSession,
  forgetHarnessSession,
  isLiveHarness,
  killAllChildren,
} from "./harness";
import {
  hasInFlightSessions,
  inFlightRefs,
  isInFlightSession,
  markTurnInterrupted,
  quitWhileBusyMessage,
  wasTurnInterrupted,
  workspaceFromResumed,
  type ResumedWorkspace,
} from "./inFlight";
import { leafIds, type WorkspaceTab } from "./layout";
import { killPty } from "./pty";
import {
  projectTerminalFileIds,
  type ProjectTerminalDock,
} from "./projectTerminal";
import { sessionWorkCwd, type Session } from "./session";
import { restoreSessionCheckout } from "./fs";
import { sessionChildHarnesses } from "./handoff";
import {
  getSession,
  listInFlightSessions,
  listSessionsByProject,
  loadWorkspaceSnapshot,
  replaceInFlightSessions,
  saveWorkspaceSnapshot,
  shouldPersistSession,
  upsertSession,
  type SessionSummary,
} from "./sessionStore";
import {
  collectWorkspaceSnapshot,
  hydrateWorkspaceSnapshot,
  parseWorkspaceSnapshot,
} from "./workspaceSnapshot";
import { loadWindowTransfer } from "./windowTransferBootstrap";
import type { WindowTransferPayload } from "./windowTransfer";
import { lastProjectPath, normalizeProjectPath, sameProjectPath } from "./recents";
import type { ProjectReturnMemory } from "./projectReturn";

export type { ResumedWorkspace };
export { hasInFlightSessions };

export type BootWorkspace = {
  windowTransfer: WindowTransferPayload | null;
  resumed: ResumedWorkspace | null;
  /** Sidebar rows listed before first paint, so the rail is not empty. */
  history: SessionSummary[];
  historyCwd: string | null;
};

let resumedPromise: Promise<ResumedWorkspace | null> | null = null;
let bootPromise: Promise<BootWorkspace> | null = null;
let quitting = false;
let quitDialogOpen = false;
let bootingResumed: ResumedWorkspace | null = null;
let liveWorkspace: {
  sessions: () => Session[];
  tabs: () => WorkspaceTab[];
  activeTabId: () => string;
  projectCwd: () => string;
  projectTerminals: () => ProjectTerminalDock[];
  projectReturnMemory: () => ProjectReturnMemory;
  flush: () => void;
} | null = null;

export function isAppQuitting(): boolean {
  return quitting;
}

export function setQuitWorkspace(
  sessions: () => Session[],
  tabs: () => WorkspaceTab[],
  activeTabId: () => string,
  projectCwd: () => string,
  projectTerminals: () => ProjectTerminalDock[],
  projectReturnMemory: () => ProjectReturnMemory,
  flush: () => void,
): () => void {
  liveWorkspace = {
    sessions,
    tabs,
    activeTabId,
    projectCwd,
    projectTerminals,
    projectReturnMemory,
    flush,
  };
  bootingResumed = null;
  return () => {
    if (liveWorkspace?.sessions === sessions) liveWorkspace = null;
  };
}

/**
 * Persist this window for a quit the coordinator has already confirmed.
 * Exiting is `confirm_quit`'s job, once every window has reported ready.
 */
export async function handleQuitRequested(): Promise<boolean> {
  if (liveWorkspace) {
    liveWorkspace.flush();
    quitting = true;
    try {
      await persistQuitState(
        liveWorkspace.sessions(),
        liveWorkspace.tabs(),
        liveWorkspace.activeTabId(),
        liveWorkspace.projectCwd(),
        liveWorkspace.projectReturnMemory(),
        "quit",
        liveWorkspace.projectTerminals(),
      );
      return true;
    } catch {
      quitting = false;
      return false;
    }
  }
  const { resumed } = await loadBootWorkspace();
  const pending = resumed ?? bootingResumed;
  quitting = true;
  if (!pending) return true;
  try {
    await persistBootingResume(pending);
    return true;
  } catch {
    quitting = false;
    return false;
  }
}

/** Each window counts its own live turns; Rust sums them into one decision. */
export async function reportQuitPoll(id: number): Promise<void> {
  let inFlight = 0;
  if (liveWorkspace) {
    liveWorkspace.flush();
    // Every running turn, not just the resumable ones `inFlightRefs` keeps:
    // an Inbox Ask still counts as work nobody agreed to throw away.
    inFlight = liveWorkspace.sessions().filter(isInFlightSession).length;
  }
  await invoke("quit_poll_reply", { id, inFlight }).catch(() => undefined);
}

/** The one quit dialog, shown by whichever window the coordinator picked. */
export async function askQuitConfirmation(
  id: number,
  inFlight: number,
): Promise<void> {
  let confirmed = false;
  if (!quitDialogOpen) {
    quitDialogOpen = true;
    try {
      confirmed = await ask(quitWhileBusyMessage(inFlight), {
        title: "MonoCode",
        kind: "warning",
        okLabel: "Quit",
      });
    } catch {
      confirmed = false;
    } finally {
      quitDialogOpen = false;
    }
  }
  await invoke("quit_decision", { id, confirmed }).catch(() => undefined);
}

export async function commitQuit(id: number): Promise<void> {
  const persisted = await handleQuitRequested();
  await invoke("quit_ready", { id, persisted }).catch(() => undefined);
}

/**
 * A quit that stopped part-way because another window could not save. This
 * window is staying open, so it must go back to persisting on unload.
 */
export function abortQuit(): void {
  quitting = false;
}

/** Confirm and stop this window's work without terminating other windows. */
export async function closeBusyWindow(): Promise<void> {
  if (!liveWorkspace) return;
  liveWorkspace.flush();
  await confirmAndCloseWindow(
    liveWorkspace.sessions(),
    liveWorkspace.tabs(),
    liveWorkspace.activeTabId(),
    liveWorkspace.projectCwd(),
    liveWorkspace.projectReturnMemory(),
    liveWorkspace.projectTerminals(),
  );
}

export function loadResumedWorkspace(): Promise<ResumedWorkspace | null> {
  if (!resumedPromise) resumedPromise = loadResumedWorkspaceOnce();
  return resumedPromise;
}

/** Transfer and restore run once; callers share the same promise. */
export function loadBootWorkspace(): Promise<BootWorkspace> {
  if (!bootPromise) {
    bootPromise = (async () => {
      const hintedCwd = lastProjectPath();
      const historyHint = listProjectHistory(hintedCwd);
      const windowTransfer = await loadWindowTransfer();
      if (windowTransfer) {
        const listed = await historyForCwd(
          windowTransfer.projectCwd,
          hintedCwd,
          historyHint,
        );
        return {
          windowTransfer,
          resumed: null,
          history: listed?.rows ?? [],
          historyCwd: listed?.cwd ?? null,
        };
      }
      const [resumed, hinted] = await Promise.all([
        loadResumedWorkspace(),
        historyHint,
      ]);
      const listed = await historyForCwd(
        resumed?.projectCwd ?? hintedCwd,
        hintedCwd,
        Promise.resolve(hinted),
      );
      return {
        windowTransfer: null,
        resumed,
        history: listed?.rows ?? [],
        historyCwd: listed?.cwd ?? null,
      };
    })();
  }
  return bootPromise;
}

async function listProjectHistory(
  cwd: string | null | undefined,
): Promise<{ cwd: string; rows: SessionSummary[] } | null> {
  if (!cwd || cwd === "~") return null;
  try {
    const rows = await listSessionsByProject(cwd);
    return { cwd: normalizeProjectPath(cwd), rows };
  } catch {
    return null;
  }
}

async function historyForCwd(
  cwd: string | null | undefined,
  hintedCwd: string | null | undefined,
  hinted: Promise<{ cwd: string; rows: SessionSummary[] } | null>,
): Promise<{ cwd: string; rows: SessionSummary[] } | null> {
  if (!cwd || cwd === "~") return null;
  if (hintedCwd && sameProjectPath(cwd, hintedCwd)) return hinted;
  return listProjectHistory(cwd);
}

async function loadResumedWorkspaceOnce(): Promise<ResumedWorkspace | null> {
  const [snapshotRaw, refs] = await Promise.all([
    loadWorkspaceSnapshot().catch(() => null),
    listInFlightSessions().catch(() => []),
  ]);
  const interrupted = new Set(refs.map((ref) => ref.sessionId));
  const snapshot = parseWorkspaceSnapshot(snapshotRaw);

  const ids = new Set<string>();
  if (snapshot) {
    for (const stub of snapshot.sessions) ids.add(stub.id);
    for (const tab of snapshot.tabs) {
      for (const id of leafIds(tab.layout)) ids.add(id);
    }
  }
  for (const ref of refs) ids.add(ref.sessionId);

  const loaded = new Map<string, Session>();
  await Promise.all(
    [...ids].map(async (id) => {
      const record = await getSession(id).catch(() => null);
      if (record) loaded.set(id, record);
    }),
  );

  let workspace = snapshot
    ? hydrateWorkspaceSnapshot(snapshot, loaded, interrupted)
    : null;
  if (!workspace && refs.length > 0) {
    const sessions: Session[] = [];
    for (const ref of refs) {
      const record = loaded.get(ref.sessionId);
      if (!record) continue;
      sessions.push(markTurnInterrupted(record));
    }
    workspace = workspaceFromResumed(sessions);
  }

  if (workspace) {
    workspace = {
      ...workspace,
      sessions: await Promise.all(
        workspace.sessions.map((session) => restoreSessionCheckout(session)),
      ),
    };
  }

  bootingResumed = workspace;
  if (workspace) {
    await Promise.all(
      workspace.sessions
        .filter(shouldPersistSession)
        .map((session) => upsertSession(session).catch(() => null)),
    );
  }
  return workspace;
}

export function bindResumedSessions(sessions: Session[]): void {
  for (const session of sessions) {
    if (!session.providerSessionId || !isLiveHarness(session.harness)) continue;
    bindHarnessSession(
      session.harness,
      session.id,
      session.providerSessionId,
      sessionWorkCwd(session),
      session.providerAccountId,
    );
  }
}

export async function hideCurrentWindow(): Promise<void> {
  await invoke("hide_window");
}

export async function closeCurrentWindow(): Promise<void> {
  await invoke("destroy_window");
}

export async function persistLiveTranscripts(
  sessions: Session[],
): Promise<void> {
  await Promise.all(
    sessions
      .filter(shouldPersistSession)
      .map((session) => upsertSession(session).catch(() => null)),
  );
}

export async function persistQuitState(
  sessions: Session[],
  tabs: WorkspaceTab[],
  activeTabId: string,
  projectCwd: string,
  memory: ProjectReturnMemory,
  mode: "quit" | "unload" = "quit",
  projectTerminals: ProjectTerminalDock[] = [],
): Promise<void> {
  const refs = inFlightRefs(sessions, tabs);
  const interrupted = new Set(refs.map((ref) => ref.sessionId));
  // A quit ends the process, so a swallowed write is work that never comes
  // back: let it reject and let the caller call the quit off. An unload is a
  // reload, where best effort is enough and failing loudly helps nobody.
  const write = <T,>(pending: Promise<T>): Promise<T | null> =>
    mode === "quit" ? pending : pending.catch(() => null);

  await Promise.all(
    sessions.map(async (session) => {
      if (!shouldPersistSession(session)) return;
      const payload = interrupted.has(session.id)
        ? markTurnInterrupted(session)
        : session;
      await write(upsertSession(payload));
    }),
  );
  await write(
    saveWorkspaceSnapshot(
      collectWorkspaceSnapshot(
        tabs,
        sessions,
        activeTabId,
        projectCwd,
        memory,
        projectTerminals,
      ),
    ),
  );
  // Vite/webview reload must not wipe a restored snapshot: those chats are idle
  // in this process until Continue runs.
  if (mode === "quit" || refs.length > 0) {
    await write(replaceInFlightSessions(refs));
  }
}

async function persistBootingResume(workspace: ResumedWorkspace): Promise<void> {
  await Promise.all(
    workspace.sessions
      .filter(shouldPersistSession)
      .map((session) => upsertSession(session).catch(() => null)),
  );
  await saveWorkspaceSnapshot(
    collectWorkspaceSnapshot(
      workspace.tabs,
      workspace.sessions,
      workspace.activeTabId,
      workspace.projectCwd,
      workspace.projectReturnMemory ?? new Map(),
      workspace.projectTerminals ?? [],
    ),
  ).catch(() => undefined);
  await replaceInFlightSessions(
    workspace.sessions
      .filter(wasTurnInterrupted)
      .map((session) => ({
        sessionId: session.id,
        cwd: session.cwd,
      })),
  ).catch(() => undefined);
}

async function confirmAndCloseWindow(
  sessions: Session[],
  tabs: WorkspaceTab[],
  activeTabId: string,
  projectCwd: string,
  memory: ProjectReturnMemory,
  projectTerminals: ProjectTerminalDock[] = [],
): Promise<void> {
  if (quitDialogOpen) return;
  quitDialogOpen = true;
  try {
    const refs = inFlightRefs(sessions, tabs);
    if (refs.length > 0) {
      const ok = await ask(
        "Close this window and stop its running chats? Other windows will stay open.",
        { title: "MonoCode", kind: "warning", okLabel: "Close window" },
      );
      if (!ok) return;
    }
    quitting = true;
    try {
      await persistQuitState(
        sessions,
        tabs,
        activeTabId,
        projectCwd,
        memory,
        "quit",
        projectTerminals,
      );
      await reapWindowRuntime(sessions, tabs, projectTerminals, false);
      await closeCurrentWindow();
    } catch {
      quitting = false;
    }
  } finally {
    quitDialogOpen = false;
  }
}

export async function reapWindowRuntime(
  sessions: Session[],
  tabs: WorkspaceTab[],
  projectTerminals: ProjectTerminalDock[] = [],
  includeAllChildren = true,
): Promise<void> {
  await Promise.all(
    sessions.map((session) =>
      Promise.all(
        sessionChildHarnesses(session).map((harness) =>
          forgetHarnessSession(harness, session.id),
        ),
      ),
    ),
  );
  await Promise.all(
    [...terminalFileIds(tabs), ...projectTerminalFileIds(projectTerminals)].map(
      (id) => killPty(id),
    ),
  );
  // Catalog probes, title generators, and usage scrapers are not session
  // children. Drop them so an unused Pi/Codex probe cannot outlive the window.
  if (includeAllChildren) await killAllChildren().catch(() => undefined);
}

function terminalFileIds(tabs: WorkspaceTab[]): string[] {
  const ids: string[] = [];
  for (const tab of tabs) {
    for (const pane of [...tab.editorPanes, ...(tab.terminalPanes ?? [])]) {
      for (const file of pane.files) {
        if (file.terminal) ids.push(file.id);
      }
    }
  }
  return ids;
}
