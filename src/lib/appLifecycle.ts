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
  loadWorkspaceSnapshot,
  replaceInFlightSessions,
  saveWorkspaceSnapshot,
  shouldPersistSession,
  upsertSession,
} from "./sessionStore";
import {
  collectWorkspaceSnapshot,
  hydrateWorkspaceSnapshot,
  parseWorkspaceSnapshot,
} from "./workspaceSnapshot";

export type { ResumedWorkspace };
export { hasInFlightSessions };

let resumedPromise: Promise<ResumedWorkspace | null> | null = null;
let quitting = false;
let quitDialogOpen = false;
let bootingResumed: ResumedWorkspace | null = null;
let liveWorkspace: {
  sessions: () => Session[];
  tabs: () => WorkspaceTab[];
  activeTabId: () => string;
  projectCwd: () => string;
  projectTerminals: () => ProjectTerminalDock[];
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
  flush: () => void,
): () => void {
  liveWorkspace = {
    sessions,
    tabs,
    activeTabId,
    projectCwd,
    projectTerminals,
    flush,
  };
  bootingResumed = null;
  return () => {
    if (liveWorkspace?.sessions === sessions) liveWorkspace = null;
  };
}

export async function handleQuitRequested(): Promise<void> {
  if (liveWorkspace) {
    liveWorkspace.flush();
    await confirmQuitAndExit(
      liveWorkspace.sessions(),
      liveWorkspace.tabs(),
      liveWorkspace.activeTabId(),
      liveWorkspace.projectCwd(),
      liveWorkspace.projectTerminals(),
    );
    return;
  }
  if (bootingResumed) {
    quitting = true;
    try {
      await persistBootingResume(bootingResumed);
      await invoke("confirm_quit");
    } catch {
      quitting = false;
    }
    return;
  }
  await invoke("confirm_quit");
}

export function loadResumedWorkspace(): Promise<ResumedWorkspace | null> {
  if (!resumedPromise) resumedPromise = loadResumedWorkspaceOnce();
  return resumedPromise;
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
  mode: "quit" | "unload" = "quit",
  projectTerminals: ProjectTerminalDock[] = [],
): Promise<void> {
  const refs = inFlightRefs(sessions, tabs);
  const interrupted = new Set(refs.map((ref) => ref.sessionId));
  await Promise.all(
    sessions.map(async (session) => {
      if (!shouldPersistSession(session)) return;
      const payload = interrupted.has(session.id)
        ? markTurnInterrupted(session)
        : session;
      await upsertSession(payload).catch(() => null);
    }),
  );
  await saveWorkspaceSnapshot(
    collectWorkspaceSnapshot(
      tabs,
      sessions,
      activeTabId,
      projectCwd,
      projectTerminals,
    ),
  ).catch(() => undefined);
  // Vite/webview reload must not wipe a restored snapshot: those chats are idle
  // in this process until Continue runs.
  if (mode === "quit" || refs.length > 0) {
    await replaceInFlightSessions(refs).catch(() => undefined);
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

async function confirmQuitAndExit(
  sessions: Session[],
  tabs: WorkspaceTab[],
  activeTabId: string,
  projectCwd: string,
  projectTerminals: ProjectTerminalDock[] = [],
): Promise<void> {
  if (quitDialogOpen) return;
  quitDialogOpen = true;
  try {
    const refs = inFlightRefs(sessions, tabs);
    if (refs.length > 0) {
      const ok = await ask(quitWhileBusyMessage(refs.length), {
        title: "MonoCode",
        kind: "warning",
        okLabel: "Quit",
      });
      if (!ok) return;
    }
    quitting = true;
    try {
      await persistQuitState(
        sessions,
        tabs,
        activeTabId,
        projectCwd,
        "quit",
        projectTerminals,
      );
      await invoke("confirm_quit");
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
  await killAllChildren().catch(() => undefined);
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
