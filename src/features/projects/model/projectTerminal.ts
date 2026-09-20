import type { CSSProperties } from "react";
import {
  newEditorPane,
  nextTerminalTitleFromFiles,
  type EditorPane,
  type FilePaneTab,
  type WorkspaceTab,
} from "../../workspace/model/layout";
import {
  normalizeProjectPath,
  sameProjectPath,
} from "./recents";
import type { Session } from "../../sessions/model/session";
import {
  applyTerminalMeta,
  type TerminalMetaPatch,
} from "../../terminal/model/terminalTab";
import { workspaceTabCwd } from "../../workspace/model/workspaceTabGroups";

export type DockSide = "top" | "bottom" | "left" | "right";

export type ProjectTerminalDock = {
  projectPath: string;
  pane: EditorPane;
  side: DockSide;
  size: number;
  open: boolean;
};

export const DOCK_SIZE_DEFAULT = {
  top: 220,
  bottom: 220,
  left: 360,
  right: 360,
} as const;

const VERTICAL_MIN = 88;
const HORIZONTAL_MIN = 180;

export function isDockSide(value: unknown): value is DockSide {
  return (
    value === "top" ||
    value === "bottom" ||
    value === "left" ||
    value === "right"
  );
}

export function isVerticalDock(side: DockSide): boolean {
  return side === "top" || side === "bottom";
}

export function defaultDockSize(side: DockSide): number {
  return DOCK_SIZE_DEFAULT[side];
}

export function clampDockSize(
  side: DockSide,
  value: number,
  viewport: { width: number; height: number } = {
    width: 1280,
    height: 800,
  },
): number {
  const vertical = isVerticalDock(side);
  const min = vertical ? VERTICAL_MIN : HORIZONTAL_MIN;
  const span = vertical ? viewport.height : viewport.width;
  const max = Math.max(min, Math.floor(span * 0.7));
  if (!Number.isFinite(value)) return defaultDockSize(side);
  return Math.min(max, Math.max(min, Math.round(value)));
}

export function findProjectTerminal(
  docks: ProjectTerminalDock[],
  projectPath: string,
): ProjectTerminalDock | undefined {
  return docks.find((dock) => sameProjectPath(dock.projectPath, projectPath));
}

export function createProjectTerminal(
  projectPath: string,
  file: FilePaneTab,
  side: DockSide = "bottom",
): ProjectTerminalDock {
  return {
    projectPath: normalizeProjectPath(projectPath),
    pane: newEditorPane(file),
    side,
    size: defaultDockSize(side),
    open: true,
  };
}

export function addTerminalToDock(
  dock: ProjectTerminalDock,
  file: FilePaneTab,
): ProjectTerminalDock {
  return {
    ...dock,
    open: true,
    pane: {
      ...dock.pane,
      files: [...dock.pane.files, file],
      activeFileId: file.id,
    },
  };
}

export function nextDockTerminalTitle(
  dock: ProjectTerminalDock,
  cwd: string,
): string {
  return nextTerminalTitleFromFiles(dock.pane.files, cwd);
}

export function closeTerminalInDock(
  dock: ProjectTerminalDock,
  fileId: string,
): ProjectTerminalDock | null {
  const index = dock.pane.files.findIndex((file) => file.id === fileId);
  if (index < 0) return dock;
  const files = dock.pane.files.filter((file) => file.id !== fileId);
  if (files.length === 0) return null;
  const activeFileId =
    dock.pane.activeFileId === fileId
      ? files[Math.min(index, files.length - 1)].id
      : dock.pane.activeFileId;
  return { ...dock, pane: { ...dock.pane, files, activeFileId } };
}

export function selectDockTerminal(
  dock: ProjectTerminalDock,
  fileId: string,
): ProjectTerminalDock {
  if (
    !dock.pane.files.some((file) => file.id === fileId) ||
    dock.pane.activeFileId === fileId
  ) {
    return dock;
  }
  return { ...dock, pane: { ...dock.pane, activeFileId: fileId } };
}

export function reorderDockTerminals(
  dock: ProjectTerminalDock,
  files: FilePaneTab[],
): ProjectTerminalDock {
  return { ...dock, pane: { ...dock.pane, files } };
}

export function patchDockTerminal(
  dock: ProjectTerminalDock,
  fileId: string,
  patch: TerminalMetaPatch,
): ProjectTerminalDock {
  let changed = false;
  const files = dock.pane.files.map((file) => {
    if (!file.terminal || file.id !== fileId) return file;
    const next = applyTerminalMeta(file, patch);
    if (next !== file) changed = true;
    return next;
  });
  if (!changed) return dock;
  return { ...dock, pane: { ...dock.pane, files } };
}

export function patchProjectTerminals(
  docks: ProjectTerminalDock[],
  fileId: string,
  patch: TerminalMetaPatch,
): ProjectTerminalDock[] {
  let changed = false;
  const next = docks.map((dock) => {
    const updated = patchDockTerminal(dock, fileId, patch);
    if (updated !== dock) changed = true;
    return updated;
  });
  return changed ? next : docks;
}

export function mapProjectTerminal(
  docks: ProjectTerminalDock[],
  projectPath: string,
  update: (dock: ProjectTerminalDock) => ProjectTerminalDock | null,
): ProjectTerminalDock[] {
  let found = false;
  const next: ProjectTerminalDock[] = [];
  for (const dock of docks) {
    if (!sameProjectPath(dock.projectPath, projectPath)) {
      next.push(dock);
      continue;
    }
    found = true;
    const updated = update(dock);
    if (updated) next.push(updated);
  }
  return found ? next : docks;
}

export function withDockOpen(
  dock: ProjectTerminalDock,
  open: boolean,
): ProjectTerminalDock {
  return dock.open === open ? dock : { ...dock, open };
}

export function withDockSide(
  dock: ProjectTerminalDock,
  side: DockSide,
  viewport?: { width: number; height: number },
): ProjectTerminalDock {
  if (dock.side === side) return dock;
  return {
    ...dock,
    side,
    size: clampDockSize(side, dock.size, viewport),
  };
}

export function withDockSize(
  dock: ProjectTerminalDock,
  size: number,
  viewport?: { width: number; height: number },
): ProjectTerminalDock {
  const next = clampDockSize(dock.side, size, viewport);
  return next === dock.size ? dock : { ...dock, size: next };
}

export function projectTerminalFileIds(
  docks: ProjectTerminalDock[],
): string[] {
  const ids: string[] = [];
  for (const dock of docks) {
    for (const file of dock.pane.files) {
      if (file.terminal) ids.push(file.id);
    }
  }
  return ids;
}

export function dockGridStyle(
  side: DockSide | null,
  size: number,
): CSSProperties {
  if (!side) {
    return {
      gridTemplateRows: "minmax(0, 1fr)",
      gridTemplateColumns: "minmax(0, 1fr)",
      gridTemplateAreas: '"main"',
    };
  }
  const px = `${Math.max(1, Math.round(size))}px`;
  if (side === "top") {
    return {
      gridTemplateRows: `${px} minmax(0, 1fr)`,
      gridTemplateColumns: "minmax(0, 1fr)",
      gridTemplateAreas: '"dock" "main"',
    };
  }
  if (side === "bottom") {
    return {
      gridTemplateRows: `minmax(0, 1fr) ${px}`,
      gridTemplateColumns: "minmax(0, 1fr)",
      gridTemplateAreas: '"main" "dock"',
    };
  }
  if (side === "left") {
    return {
      gridTemplateRows: "minmax(0, 1fr)",
      gridTemplateColumns: `${px} minmax(0, 1fr)`,
      gridTemplateAreas: '"dock main"',
    };
  }
  return {
    gridTemplateRows: "minmax(0, 1fr)",
    gridTemplateColumns: `minmax(0, 1fr) ${px}`,
    gridTemplateAreas: '"main dock"',
  };
}

export function applyDockGridStyle(
  el: HTMLElement,
  side: DockSide | null,
  size: number,
): void {
  const style = dockGridStyle(side, size);
  el.style.gridTemplateRows = String(style.gridTemplateRows ?? "");
  el.style.gridTemplateColumns = String(style.gridTemplateColumns ?? "");
  el.style.gridTemplateAreas = String(style.gridTemplateAreas ?? "");
}

/**
 * A dock follows the tabs of its project into a new window only when
 * every remaining tab of that project is leaving too — otherwise the
 * original window keeps the running terminals.
 */
export function splitProjectTerminalsForMove(
  docks: ProjectTerminalDock[],
  movingTabs: WorkspaceTab[],
  remainingTabs: WorkspaceTab[],
  sessions: Session[],
): { moving: ProjectTerminalDock[]; remaining: ProjectTerminalDock[] } {
  const remainingProjects = projectPathsOf(remainingTabs, sessions);
  const movingProjects = projectPathsOf(movingTabs, sessions);
  const moving: ProjectTerminalDock[] = [];
  const remaining: ProjectTerminalDock[] = [];
  for (const dock of docks) {
    const path = normalizeProjectPath(dock.projectPath);
    const stays = remainingProjects.has(path);
    const follows = movingProjects.has(path) && !stays;
    if (follows) moving.push(dock);
    else remaining.push(dock);
  }
  return { moving, remaining };
}

function projectPathsOf(
  tabs: WorkspaceTab[],
  sessions: Session[],
): Set<string> {
  const paths = new Set<string>();
  for (const tab of tabs) {
    const cwd = workspaceTabCwd(tab, sessions);
    if (cwd) paths.add(normalizeProjectPath(cwd));
  }
  return paths;
}
