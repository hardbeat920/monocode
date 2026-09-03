import { markTurnInterrupted, type ResumedWorkspace } from "./inFlight";
import {
  isTerminalTab,
  leafIds,
  newTab,
  type CommitTabSource,
  type EditorPane,
  type FilePaneTab,
  type LayoutNode,
  type PlanTabSource,
  type WorkspaceTab,
} from "./layout";
import type { ReleaseNotesTabSource } from "./releaseNotes";
import {
  clampDockSize,
  isDockSide,
  type ProjectTerminalDock,
} from "./projectTerminal";
import { normalizeProjectPath } from "./recents";
import {
  HARNESSES,
  RUNTIME_MODES,
  newSession,
  type HarnessId,
  type RuntimeMode,
  type Session,
} from "./session";

export type WorkspaceSessionStub = {
  id: string;
  cwd: string;
  harness: HarnessId;
  model: string;
  modelSettings: Record<string, string>;
  runtimeMode: RuntimeMode;
  title: string;
  providerSessionId?: string;
  branch?: string;
  worktreeCwd?: string;
};

export type WorkspaceSnapshot = {
  tabs: WorkspaceTab[];
  sessions: WorkspaceSessionStub[];
  activeTabId: string;
  projectCwd: string;
  projectTerminals: ProjectTerminalDock[];
};

export function collectWorkspaceSnapshot(
  tabs: WorkspaceTab[],
  sessions: Session[],
  activeTabId: string,
  projectCwd: string,
  projectTerminals: ProjectTerminalDock[] = [],
): WorkspaceSnapshot {
  return {
    tabs: tabs.map(sanitizeTab).filter((tab): tab is WorkspaceTab => tab != null),
    sessions: sessions.map(sessionStub).filter((stub): stub is WorkspaceSessionStub => stub != null),
    activeTabId,
    projectCwd: projectCwd.trim() || "~",
    projectTerminals: projectTerminals
      .map(sanitizeProjectTerminal)
      .filter((dock): dock is ProjectTerminalDock => dock != null),
  };
}

export function parseWorkspaceSnapshot(raw: unknown): WorkspaceSnapshot | null {
  if (!raw || typeof raw !== "object") return null;
  const value = raw as {
    tabs?: unknown;
    sessions?: unknown;
    activeTabId?: unknown;
    projectCwd?: unknown;
    projectTerminals?: unknown;
  };
  if (!Array.isArray(value.tabs) || typeof value.activeTabId !== "string") {
    return null;
  }
  const tabs = value.tabs
    .map(sanitizeTab)
    .filter((tab): tab is WorkspaceTab => tab != null);
  if (tabs.length === 0) return null;
  const sessions = Array.isArray(value.sessions)
    ? value.sessions
        .map(sanitizeStub)
        .filter((stub): stub is WorkspaceSessionStub => stub != null)
    : [];
  const activeTabId = tabs.some((tab) => tab.id === value.activeTabId)
    ? value.activeTabId
    : tabs[0].id;
  const projectCwd =
    typeof value.projectCwd === "string" && value.projectCwd.trim()
      ? value.projectCwd.trim()
      : "~";
  const projectTerminals = Array.isArray(value.projectTerminals)
    ? value.projectTerminals
        .map(sanitizeProjectTerminal)
        .filter((dock): dock is ProjectTerminalDock => dock != null)
    : [];
  return { tabs, sessions, activeTabId, projectCwd, projectTerminals };
}

export function workspaceSnapshotKey(snapshot: WorkspaceSnapshot): string {
  return JSON.stringify(snapshot);
}

/**
 * Reopen the saved tabs/panes. Transcripts come from `loaded` when the
 * session was persisted; blank tabs fall back to the stub.
 */
export function hydrateWorkspaceSnapshot(
  snapshot: WorkspaceSnapshot,
  loaded: Map<string, Session>,
  interruptedIds: ReadonlySet<string> = new Set(),
): ResumedWorkspace | null {
  const parsed = parseWorkspaceSnapshot(snapshot);
  if (!parsed) return null;

  const paneIds = new Set<string>();
  for (const tab of parsed.tabs) {
    for (const pane of [...tab.editorPanes, ...(tab.terminalPanes ?? [])]) {
      paneIds.add(pane.id);
    }
  }

  const stubs = new Map(parsed.sessions.map((stub) => [stub.id, stub]));
  const sessions = new Map<string, Session>();

  const take = (id: string): Session | null => {
    const existing = sessions.get(id);
    if (existing) return existing;
    const record = loaded.get(id);
    const stub = stubs.get(id);
    const base = record ?? (stub ? sessionFromStub(stub) : null);
    if (!base) return null;
    const next = interruptedIds.has(id) ? markTurnInterrupted(base) : { ...base, busy: false };
    sessions.set(id, next);
    return next;
  };

  for (const stub of parsed.sessions) take(stub.id);

  const tabs: WorkspaceTab[] = [];
  for (const tab of parsed.tabs) {
    for (const id of leafIds(tab.layout)) {
      if (paneIds.has(id) || take(id)) continue;
      sessions.set(
        id,
        sessionFromStub({
          id,
          cwd: parsed.projectCwd,
          harness: "cursor",
          model: "",
          modelSettings: {},
          runtimeMode: "supervised",
          title: "",
        }),
      );
    }
    tabs.push(tab);
  }

  for (const id of interruptedIds) {
    if (!take(id)) continue;
    if (tabs.some((tab) => leafIds(tab.layout).includes(id))) continue;
    tabs.push(newTab(id));
  }

  if (tabs.length === 0) return null;
  const activeTabId = tabs.some((tab) => tab.id === parsed.activeTabId)
    ? parsed.activeTabId
    : tabs[0].id;
  const projectCwd =
    parsed.projectCwd !== "~"
      ? parsed.projectCwd
      : sessions.values().next().value?.cwd ?? "~";

  return {
    tabs,
    sessions: [...sessions.values()],
    activeTabId,
    projectCwd,
    projectTerminals: parsed.projectTerminals,
  };
}

function sessionStub(session: Session): WorkspaceSessionStub | null {
  if (!session.id) return null;
  return {
    id: session.id,
    cwd: session.cwd || "~",
    harness: session.harness,
    model: session.model,
    modelSettings: { ...session.modelSettings },
    runtimeMode: session.runtimeMode,
    title: session.title,
    ...(session.providerSessionId
      ? { providerSessionId: session.providerSessionId }
      : {}),
    ...(session.branch ? { branch: session.branch } : {}),
    ...(session.worktreeCwd ? { worktreeCwd: session.worktreeCwd } : {}),
  };
}

function sessionFromStub(stub: WorkspaceSessionStub): Session {
  const session = newSession(
    stub.harness,
    stub.cwd,
    stub.model,
    stub.runtimeMode,
    stub.modelSettings,
  );
  return {
    ...session,
    id: stub.id,
    title: stub.title,
    ...(stub.providerSessionId
      ? { providerSessionId: stub.providerSessionId }
      : {}),
    ...(stub.branch ? { branch: stub.branch } : {}),
    ...(stub.worktreeCwd ? { worktreeCwd: stub.worktreeCwd } : {}),
  };
}

function sanitizeStub(raw: unknown): WorkspaceSessionStub | null {
  if (!raw || typeof raw !== "object") return null;
  const value = raw as Record<string, unknown>;
  if (typeof value.id !== "string" || !value.id) return null;
  const harness = asHarness(value.harness);
  const runtimeMode = asRuntimeMode(value.runtimeMode);
  if (!harness || !runtimeMode) return null;
  const modelSettings =
    value.modelSettings &&
    typeof value.modelSettings === "object" &&
    !Array.isArray(value.modelSettings)
      ? Object.fromEntries(
          Object.entries(value.modelSettings as Record<string, unknown>).filter(
            (entry): entry is [string, string] => typeof entry[1] === "string",
          ),
        )
      : {};
  return {
    id: value.id,
    cwd:
      typeof value.cwd === "string" && value.cwd.trim() ? value.cwd.trim() : "~",
    harness,
    model: typeof value.model === "string" ? value.model : "",
    modelSettings,
    runtimeMode,
    title: typeof value.title === "string" ? value.title : "",
    ...(typeof value.providerSessionId === "string" && value.providerSessionId
      ? { providerSessionId: value.providerSessionId }
      : {}),
    ...(typeof value.branch === "string" && value.branch.trim()
      ? { branch: value.branch.trim() }
      : {}),
    ...(typeof value.worktreeCwd === "string" && value.worktreeCwd.trim()
      ? { worktreeCwd: value.worktreeCwd.trim() }
      : {}),
  };
}

function sanitizeTab(raw: unknown): WorkspaceTab | null {
  if (!raw || typeof raw !== "object") return null;
  const value = raw as Record<string, unknown>;
  if (typeof value.id !== "string" || !value.id) return null;
  const layout = sanitizeLayout(value.layout);
  if (!layout) return null;
  const editorResult = sanitizePanes(value.editorPanes);
  const terminalResult = sanitizePanes(value.terminalPanes);
  const invalidPaneIds = new Set([
    ...editorResult.invalidIds,
    ...terminalResult.invalidIds,
  ]);
  if (leafIds(layout).some((id) => invalidPaneIds.has(id))) return null;
  const editorPanes = editorResult.panes;
  const terminalPanes = terminalResult.panes;
  const focusedId =
    typeof value.focusedId === "string" && value.focusedId
      ? value.focusedId
      : leafIds(layout)[0];
  if (!focusedId) return null;
  return {
    kind: "session",
    id: value.id,
    layout,
    focusedId,
    editorPanes,
    terminalPanes,
    ...(value.diffOpen === true ? { diffOpen: true } : {}),
    ...(value.diffFocused === true ? { diffFocused: true } : {}),
    ...(typeof value.groupId === "string" && value.groupId
      ? { groupId: value.groupId }
      : {}),
  };
}

function sanitizeLayout(raw: unknown): LayoutNode | null {
  if (!raw || typeof raw !== "object") return null;
  const value = raw as Record<string, unknown>;
  if (value.type === "leaf") {
    return typeof value.id === "string" && value.id
      ? { type: "leaf", id: value.id }
      : null;
  }
  if (value.type !== "split" || typeof value.id !== "string" || !value.id) {
    return null;
  }
  const dir = value.dir === "down" ? "down" : value.dir === "right" ? "right" : null;
  if (!dir || !Array.isArray(value.children) || value.children.length < 2) {
    return null;
  }
  const children = value.children
    .map(sanitizeLayout)
    .filter((node): node is LayoutNode => node != null);
  if (children.length < 2) return null;
  const sizes = Array.isArray(value.sizes)
    ? value.sizes.filter((size): size is number => typeof size === "number" && Number.isFinite(size))
    : [];
  const normalized =
    sizes.length === children.length
      ? sizes
      : children.map(() => 1 / children.length);
  return { type: "split", id: value.id, dir, children, sizes: normalized };
}

function sanitizePanes(raw: unknown): {
  panes: EditorPane[];
  invalidIds: string[];
} {
  if (!Array.isArray(raw)) return { panes: [], invalidIds: [] };
  const panes: EditorPane[] = [];
  const invalidIds: string[] = [];
  for (const entry of raw) {
    const pane = sanitizePane(entry);
    if (pane) {
      panes.push(pane);
      continue;
    }
    if (!entry || typeof entry !== "object") continue;
    const id = (entry as Record<string, unknown>).id;
    if (typeof id === "string" && id) invalidIds.push(id);
  }
  return { panes, invalidIds };
}

function sanitizePane(raw: unknown): EditorPane | null {
  if (!raw || typeof raw !== "object") return null;
  const value = raw as Record<string, unknown>;
  if (typeof value.id !== "string" || !value.id) return null;
  if (!Array.isArray(value.files)) return null;
  const files = value.files
    .map(sanitizeFile)
    .filter((file): file is FilePaneTab => file != null);
  if (files.length === 0) return null;
  const activeFileId =
    typeof value.activeFileId === "string" &&
    files.some((file) => file.id === value.activeFileId)
      ? value.activeFileId
      : files[0].id;
  return { id: value.id, files, activeFileId };
}

function sanitizeFile(raw: unknown): FilePaneTab | null {
  if (!raw || typeof raw !== "object") return null;
  const value = raw as Record<string, unknown>;
  if (typeof value.id !== "string" || !value.id) return null;
  if (typeof value.path !== "string" || !value.path) return null;
  if (typeof value.cwd !== "string" || !value.cwd) return null;
  const plan = sanitizePlan(value.plan);
  const hasReleaseNotes = "releaseNotes" in value;
  const releaseNotes = sanitizeReleaseNotes(value.releaseNotes);
  const hasCommit = "commit" in value;
  const commit = sanitizeCommit(value.commit);
  if (hasReleaseNotes && !releaseNotes) return null;
  if (hasCommit && !commit) return null;
  if (
    releaseNotes &&
    (value.plan != null ||
      value.review === true ||
      value.changes === true ||
      value.terminal === true ||
      commit != null)
  ) {
    return null;
  }
  if (
    commit &&
    (value.plan != null ||
      value.review === true ||
      value.changes === true ||
      value.terminal === true)
  ) {
    return null;
  }
  return {
    id: value.id,
    path: value.path,
    cwd: value.cwd,
    ...(plan ? { plan } : {}),
    ...(releaseNotes ? { releaseNotes } : {}),
    ...(commit ? { commit } : {}),
    ...(value.review === true ? { review: true } : {}),
    ...(value.changes === true ? { changes: true, review: true } : {}),
    ...(value.terminal === true ? { terminal: true } : {}),
  };
}

function sanitizeCommit(raw: unknown): CommitTabSource | undefined {
  if (!raw || typeof raw !== "object") return undefined;
  const value = raw as Record<string, unknown>;
  if (typeof value.sha !== "string" || !value.sha.trim()) return undefined;
  if (typeof value.shortSha !== "string" || !value.shortSha.trim()) {
    return undefined;
  }
  if (typeof value.subject !== "string") return undefined;
  return {
    sha: value.sha.trim(),
    shortSha: value.shortSha.trim(),
    subject: value.subject,
  };
}

function sanitizeReleaseNotes(
  raw: unknown,
): ReleaseNotesTabSource | undefined {
  if (!raw || typeof raw !== "object") return undefined;
  const version = (raw as Record<string, unknown>).version;
  if (typeof version !== "string" || !version.trim()) return undefined;
  return { version: version.trim() };
}

function sanitizeProjectTerminal(raw: unknown): ProjectTerminalDock | null {
  if (!raw || typeof raw !== "object") return null;
  const value = raw as Record<string, unknown>;
  if (typeof value.projectPath !== "string" || !value.projectPath.trim()) {
    return null;
  }
  if (!isDockSide(value.side)) return null;
  const pane = sanitizePane(value.pane);
  if (!pane) return null;
  const files = pane.files.filter(isTerminalTab);
  if (files.length === 0) return null;
  const activeFileId = files.some((file) => file.id === pane.activeFileId)
    ? pane.activeFileId
    : files[0].id;
  return {
    projectPath: normalizeProjectPath(value.projectPath),
    pane: { ...pane, files, activeFileId },
    side: value.side,
    size: clampDockSize(value.side, Number(value.size)),
    open: value.open !== false,
  };
}

function sanitizePlan(raw: unknown): PlanTabSource | undefined {
  if (!raw || typeof raw !== "object") return undefined;
  const value = raw as Record<string, unknown>;
  if (typeof value.sessionId !== "string" || !value.sessionId) return undefined;
  if (typeof value.blockId !== "string" || !value.blockId) return undefined;
  if (typeof value.title !== "string") return undefined;
  return {
    sessionId: value.sessionId,
    blockId: value.blockId,
    title: value.title,
  };
}

function asHarness(value: unknown): HarnessId | null {
  return typeof value === "string" && (HARNESSES as string[]).includes(value)
    ? (value as HarnessId)
    : null;
}

function asRuntimeMode(value: unknown): RuntimeMode | null {
  return typeof value === "string" && (RUNTIME_MODES as string[]).includes(value)
    ? (value as RuntimeMode)
    : null;
}
