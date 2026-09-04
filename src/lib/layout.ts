import type { ReleaseNotesTabSource } from "./releaseNotes";
import {
  applyTerminalMeta,
  defaultTerminalTitle,
  type TerminalMetaPatch,
} from "./terminalTab";

/**
 * Split tree for a tab. Same-direction splits share a group so
 * new panes divide space equally until the user drags a sash.
 * Panes can be dragged onto another pane's edge: same-axis
 * siblings reorder, a perpendicular edge nests a new split, and
 * a drop in another group relocates the leaf there. Session cards
 * from the sidebar use the same edges to open or move a chat into
 * that pane. cmd-d splits right, shift-cmd-d splits down,
 * cmd-opt-arrows move focus to the adjacent pane.
 */

export type SplitDir = "right" | "down";
export type FocusDir = "left" | "right" | "up" | "down";

export type LayoutNode =
  | { type: "leaf"; id: string }
  | {
      type: "split";
      id: string;
      dir: SplitDir;
      children: LayoutNode[];
      sizes: number[];
    };

export type PlanTabSource = {
  sessionId: string;
  blockId: string;
  title: string;
};

export type CommitTabSource = {
  sha: string;
  shortSha: string;
  subject: string;
};

export type SessionChangesSource = {
  sessionId: string;
};

export type FilePaneTab = {
  id: string;
  path: string;
  cwd: string;
  plan?: PlanTabSource;
  releaseNotes?: ReleaseNotesTabSource;
  review?: boolean;
  /** Single working-tree review of every changed file (unified diff). */
  changes?: boolean;
  /** Read-only diff built from one session's captured before/after snapshots. */
  sessionChanges?: SessionChangesSource;
  /** Historical commit review (unified diff, read-only). */
  commit?: CommitTabSource;
  terminal?: boolean;
  /** Foreground command when it isn't the shell. Live only — not persisted. */
  foreground?: string;
};

export type EditorPane = {
  id: string;
  files: FilePaneTab[];
  activeFileId: string;
};

export type SurfaceKind = "editor" | "terminal";

export type WorkspaceTab = {
  kind: "session";
  id: string;
  layout: LayoutNode;
  focusedId: string;
  editorPanes: EditorPane[];
  terminalPanes: EditorPane[];
  diffOpen?: boolean;
  diffFocused?: boolean;
  /** Explicit tab group; absent means ungrouped. */
  groupId?: string;
};

const MIN_SIZE = 0.08;

export function leaf(sessionId: string): LayoutNode {
  return { type: "leaf", id: sessionId };
}

export function newTab(sessionId: string): WorkspaceTab {
  return {
    kind: "session",
    id: crypto.randomUUID(),
    layout: leaf(sessionId),
    focusedId: sessionId,
    editorPanes: [],
    terminalPanes: [],
  };
}

export function newFileTab(
  path: string,
  cwd: string,
  review = false,
): FilePaneTab {
  return {
    id: crypto.randomUUID(),
    path,
    cwd,
    ...(review ? { review: true } : {}),
  };
}

export function newChangesTab(cwd: string, focusPath?: string): FilePaneTab {
  return {
    id: crypto.randomUUID(),
    path: focusPath || cwd,
    cwd,
    review: true,
    changes: true,
  };
}

export function newSessionChangesTab(
  cwd: string,
  sessionId: string,
  focusPath?: string,
): FilePaneTab {
  return {
    id: crypto.randomUUID(),
    path: focusPath || cwd,
    cwd,
    review: true,
    sessionChanges: { sessionId },
  };
}

export function newCommitTab(cwd: string, commit: CommitTabSource): FilePaneTab {
  return {
    id: crypto.randomUUID(),
    path: `commit:${commit.sha}`,
    cwd,
    commit,
  };
}

export function newPlanTab(
  sessionId: string,
  blockId: string,
  title: string,
  cwd: string,
): FilePaneTab {
  return {
    id: crypto.randomUUID(),
    path: `plan:${blockId}`,
    cwd,
    plan: { sessionId, blockId, title },
  };
}

export function newReleaseNotesWorkspaceTab(
  releaseNotes: ReleaseNotesTabSource,
): WorkspaceTab {
  const file: FilePaneTab = {
    id: crypto.randomUUID(),
    path: `release-notes:${releaseNotes.version}`,
    cwd: "~",
    releaseNotes,
  };
  const pane = newEditorPane(file);
  return {
    kind: "session",
    id: crypto.randomUUID(),
    layout: leaf(pane.id),
    focusedId: pane.id,
    editorPanes: [pane],
    terminalPanes: [],
  };
}

export function newTerminalFile(cwd: string, title?: string): FilePaneTab {
  return {
    id: crypto.randomUUID(),
    path: title ?? defaultTerminalTitle(cwd),
    cwd,
    terminal: true,
  };
}

export function newTerminalWorkspaceTab(file: FilePaneTab): WorkspaceTab {
  const pane = newEditorPane(file);
  return {
    kind: "session",
    id: crypto.randomUUID(),
    layout: leaf(pane.id),
    focusedId: pane.id,
    editorPanes: [],
    terminalPanes: [pane],
  };
}

export function nextTerminalTitleFromFiles(
  files: Iterable<FilePaneTab>,
  cwd: string,
): string {
  const base = defaultTerminalTitle(cwd);
  const taken = new Set<string>();
  for (const file of files) {
    if (isTerminalTab(file)) taken.add(file.path);
  }
  if (!taken.has(base)) return base;
  let index = 2;
  while (taken.has(`${base} ${index}`)) index += 1;
  return `${base} ${index}`;
}

export function nextTerminalTitle(tab: WorkspaceTab, cwd: string): string {
  return nextTerminalTitleFromFiles(
    (tab.terminalPanes ?? []).flatMap((pane) => pane.files),
    cwd,
  );
}

export function updateTerminalTab(
  tab: WorkspaceTab,
  fileId: string,
  patch: TerminalMetaPatch,
): WorkspaceTab {
  const panes = tab.terminalPanes ?? [];
  let changed = false;
  const terminalPanes = panes.map((pane) => {
    const files = pane.files.map((file) => {
      if (!file.terminal || file.id !== fileId) return file;
      const next = applyTerminalMeta(file, patch);
      if (next !== file) changed = true;
      return next;
    });
    return files === pane.files ? pane : { ...pane, files };
  });
  if (!changed) return tab;
  return withSurfacePanes(tab, "terminal", terminalPanes);
}

export function surfacePanes(
  tab: WorkspaceTab,
  kind: SurfaceKind,
): EditorPane[] {
  return kind === "editor" ? tab.editorPanes : (tab.terminalPanes ?? []);
}

export function withSurfacePanes(
  tab: WorkspaceTab,
  kind: SurfaceKind,
  panes: EditorPane[],
): WorkspaceTab {
  return kind === "editor"
    ? { ...tab, editorPanes: panes }
    : { ...tab, terminalPanes: panes };
}

export function findSurfacePane(
  tab: WorkspaceTab,
  paneId: string,
): { kind: SurfaceKind; pane: EditorPane } | undefined {
  const editor = tab.editorPanes.find((pane) => pane.id === paneId);
  if (editor) return { kind: "editor", pane: editor };
  const terminal = (tab.terminalPanes ?? []).find((pane) => pane.id === paneId);
  if (terminal) return { kind: "terminal", pane: terminal };
}

export function isPlanTab(
  file: FilePaneTab,
): file is FilePaneTab & { plan: PlanTabSource } {
  return !!file.plan;
}

export function isReleaseNotesTab(
  file: FilePaneTab,
): file is FilePaneTab & { releaseNotes: ReleaseNotesTabSource } {
  return !!file.releaseNotes;
}

export function isCommitTab(
  file: FilePaneTab,
): file is FilePaneTab & { commit: CommitTabSource } {
  return !!file.commit;
}

export function isTerminalTab(file: FilePaneTab): boolean {
  return !!file.terminal;
}

export function isVirtualDocumentTab(file: FilePaneTab): boolean {
  return isPlanTab(file) || isReleaseNotesTab(file) || isCommitTab(file);
}

export function isFilesystemTab(file: FilePaneTab): boolean {
  return (
    !isTerminalTab(file) &&
    !isVirtualDocumentTab(file) &&
    !file.sessionChanges
  );
}

export function focusedFileTab(tab: WorkspaceTab): FilePaneTab | undefined {
  const pane =
    tab.editorPanes.find((entry) => entry.id === tab.focusedId) ??
    (tab.terminalPanes ?? []).find((entry) => entry.id === tab.focusedId);
  return pane?.files.find((file) => file.id === pane.activeFileId);
}

/** Move terminal tabs out of file panes so the two never share a tab strip. */
export function isolateTerminalPanes(tab: WorkspaceTab): WorkspaceTab {
  const existingTerminals = tab.terminalPanes ?? [];
  const mixed = tab.editorPanes.some((pane) => pane.files.some(isTerminalTab));
  if (!mixed && tab.terminalPanes) return tab;

  let layout = tab.layout;
  let focusedId = tab.focusedId;
  const editorPanes: EditorPane[] = [];
  const terminalPanes = [...existingTerminals];

  for (const pane of tab.editorPanes) {
    const files = pane.files.filter((file) => !isTerminalTab(file));
    const terminals = pane.files.filter(isTerminalTab);
    if (files.length > 0) {
      editorPanes.push({
        ...pane,
        files,
        activeFileId: files.some((file) => file.id === pane.activeFileId)
          ? pane.activeFileId
          : files[0].id,
      });
    }
    if (terminals.length === 0) continue;
    if (files.length === 0) {
      terminalPanes.push({ ...pane, files: terminals });
      continue;
    }
    const split: EditorPane = {
      id: crypto.randomUUID(),
      files: terminals,
      activeFileId: terminals.some((file) => file.id === pane.activeFileId)
        ? pane.activeFileId
        : terminals[0].id,
    };
    terminalPanes.push(split);
    layout = splitPane(layout, pane.id, "down", split.id);
    if (pane.id === tab.focusedId && split.activeFileId === pane.activeFileId) {
      focusedId = split.id;
    }
  }

  return { ...tab, layout, focusedId, editorPanes, terminalPanes };
}

export function isReviewTab(file: FilePaneTab): boolean {
  return !!file.review && !isVirtualDocumentTab(file);
}

export function isChangesTab(file: FilePaneTab): boolean {
  return !!file.changes && isReviewTab(file);
}

export function isSessionChangesTab(
  file: FilePaneTab,
): file is FilePaneTab & { sessionChanges: SessionChangesSource } {
  return !!file.sessionChanges && isReviewTab(file);
}

export function editorTabKey(file: FilePaneTab): string {
  if (file.terminal) return `terminal:${file.id}`;
  if (file.plan) return `plan:${file.plan.blockId}`;
  if (file.releaseNotes) return `release-notes:${file.releaseNotes.version}`;
  if (file.commit) return `commit:${file.cwd}:${file.commit.sha}`;
  if (file.sessionChanges)
    return `session-changes:${file.cwd}:${file.sessionChanges.sessionId}`;
  if (file.changes) return `changes:${file.cwd}`;
  return file.review ? `review:${file.path}` : `file:${file.path}`;
}

export function newEditorPane(file: FilePaneTab): EditorPane {
  return {
    id: crypto.randomUUID(),
    files: [file],
    activeFileId: file.id,
  };
}

/** Focus an existing editor tab, or open it in the focused editor pane / a new split. */
export function openEditorTab(
  tab: WorkspaceTab,
  file: FilePaneTab,
): WorkspaceTab {
  if (file.terminal) return openTerminalTab(tab, file);
  tab = isolateTerminalPanes(tab);

  const key = editorTabKey(file);
  const existingPane = tab.editorPanes.find((pane) =>
    pane.files.some((entry) => editorTabKey(entry) === key),
  );
  const existingFile = existingPane?.files.find(
    (entry) => editorTabKey(entry) === key,
  );
  if (existingPane && existingFile) {
    return {
      ...tab,
      focusedId: existingPane.id,
      diffFocused: false,
      editorPanes: tab.editorPanes.map((pane) =>
        pane.id === existingPane.id
          ? { ...pane, activeFileId: existingFile.id }
          : pane,
      ),
    };
  }

  const focusedPane = tab.editorPanes.find((pane) => pane.id === tab.focusedId);
  const targetPane = focusedPane ?? tab.editorPanes[0];
  if (targetPane) {
    return {
      ...tab,
      focusedId: targetPane.id,
      diffFocused: false,
      editorPanes: tab.editorPanes.map((pane) =>
        pane.id === targetPane.id
          ? {
              ...pane,
              files: [...pane.files, file],
              activeFileId: file.id,
            }
          : pane,
      ),
    };
  }

  const editorPane = newEditorPane(file);
  return {
    ...tab,
    layout: splitPane(tab.layout, tab.focusedId, "right", editorPane.id),
    focusedId: editorPane.id,
    diffFocused: false,
    editorPanes: [editorPane],
  };
}

/** Focus the single working-tree Changes tab, creating it if needed. */
export function openChangesTab(
  tab: WorkspaceTab,
  cwd: string,
  focusPath?: string,
): WorkspaceTab {
  tab = isolateTerminalPanes(tab);
  const next = newChangesTab(cwd, focusPath);
  const existingPane = tab.editorPanes.find((pane) =>
    pane.files.some(isChangesTab),
  );
  const existingFile = existingPane?.files.find(isChangesTab);

  if (existingPane && existingFile) {
    const updated = focusPath ? { ...existingFile, path: focusPath } : existingFile;
    return {
      ...tab,
      focusedId: existingPane.id,
      diffFocused: false,
      editorPanes: tab.editorPanes.map((pane) =>
        pane.id === existingPane.id
          ? {
              ...pane,
              files: dropPerFileReviewTabs(pane.files).map((file) =>
                file.id === updated.id ? updated : file,
              ),
              activeFileId: updated.id,
            }
          : pane,
      ),
    };
  }

  const opened = openEditorTab(tab, next);
  return {
    ...opened,
    editorPanes: opened.editorPanes.map((pane) =>
      pane.files.some(isChangesTab)
        ? {
            ...pane,
            files: dropPerFileReviewTabs(pane.files),
            activeFileId:
              pane.files.find(isChangesTab)?.id ?? pane.activeFileId,
          }
        : pane,
    ),
  };
}

/** Focus one session's exact captured diff, creating a tab if needed. */
export function openSessionChangesTab(
  tab: WorkspaceTab,
  cwd: string,
  sessionId: string,
  focusPath?: string,
): WorkspaceTab {
  const next = newSessionChangesTab(cwd, sessionId, focusPath);
  const key = editorTabKey(next);
  const existingPane = tab.editorPanes.find((pane) =>
    pane.files.some((file) => editorTabKey(file) === key),
  );
  const existingFile = existingPane?.files.find(
    (file) => editorTabKey(file) === key,
  );
  if (!existingPane || !existingFile) return openEditorTab(tab, next);

  const updated = focusPath ? { ...existingFile, path: focusPath } : existingFile;
  return {
    ...tab,
    focusedId: existingPane.id,
    diffFocused: false,
    editorPanes: tab.editorPanes.map((pane) =>
      pane.id === existingPane.id
        ? {
            ...pane,
            files: pane.files.map((file) =>
              file.id === updated.id ? updated : file,
            ),
            activeFileId: updated.id,
          }
        : pane,
    ),
  };
}

/** Focus a historical commit's unified diff, creating the tab if needed. */
export function openCommitTab(
  tab: WorkspaceTab,
  cwd: string,
  commit: CommitTabSource,
): WorkspaceTab {
  return openEditorTab(tab, newCommitTab(cwd, commit));
}

function dropPerFileReviewTabs(files: FilePaneTab[]): FilePaneTab[] {
  return files.filter(
    (file) =>
      !isReviewTab(file) || isChangesTab(file) || isSessionChangesTab(file),
  );
}

/** Open a terminal in its own pane. Files never share this tab strip. */
export function openTerminalTab(
  tab: WorkspaceTab,
  file: FilePaneTab,
  occupyPaneId?: string,
): WorkspaceTab {
  tab = isolateTerminalPanes(tab);
  const occupying =
    occupyPaneId &&
    !tab.editorPanes.some((pane) => pane.id === occupyPaneId) &&
    !tab.terminalPanes.some((pane) => pane.id === occupyPaneId);
  if (occupying && occupyPaneId) {
    const pane = newEditorPane(file);
    return {
      ...tab,
      layout: replaceLeafId(tab.layout, occupyPaneId, pane.id),
      focusedId: pane.id,
      diffFocused: false,
      terminalPanes: [...tab.terminalPanes, pane],
    };
  }

  const focusedPane = tab.terminalPanes.find(
    (pane) => pane.id === tab.focusedId,
  );
  const targetPane = focusedPane ?? tab.terminalPanes[0];
  if (targetPane) {
    return {
      ...tab,
      focusedId: targetPane.id,
      diffFocused: false,
      terminalPanes: tab.terminalPanes.map((pane) =>
        pane.id === targetPane.id
          ? {
              ...pane,
              files: [...pane.files, file],
              activeFileId: file.id,
            }
          : pane,
      ),
    };
  }

  const pane = newEditorPane(file);
  return {
    ...tab,
    layout: splitPane(tab.layout, tab.focusedId, "down", pane.id),
    focusedId: pane.id,
    diffFocused: false,
    terminalPanes: [pane],
  };
}

function equalSizes(n: number): number[] {
  return Array.from({ length: n }, () => 1 / n);
}

function normalize(sizes: number[]): number[] {
  const total = sizes.reduce((sum, n) => sum + n, 0);
  if (total <= 0) return equalSizes(sizes.length);
  return sizes.map((n) => n / total);
}

export function splitPane(
  node: LayoutNode,
  focusedId: string,
  dir: SplitDir,
  newSessionId: string,
): LayoutNode {
  if (node.type === "leaf") {
    if (node.id !== focusedId) return node;
    return {
      type: "split",
      id: crypto.randomUUID(),
      dir,
      children: [node, leaf(newSessionId)],
      sizes: [0.5, 0.5],
    };
  }

  const direct = node.children.findIndex(
    (child) => child.type === "leaf" && child.id === focusedId,
  );

  if (direct >= 0) {
    if (node.dir === dir) {
      const children = [
        ...node.children.slice(0, direct + 1),
        leaf(newSessionId),
        ...node.children.slice(direct + 1),
      ];
      return { ...node, children, sizes: equalSizes(children.length) };
    }
    return {
      ...node,
      children: node.children.map((child, i) =>
        i === direct
          ? {
              type: "split",
              id: crypto.randomUUID(),
              dir,
              children: [child, leaf(newSessionId)],
              sizes: [0.5, 0.5],
            }
          : child,
      ),
    };
  }

  return {
    ...node,
    children: node.children.map((child) =>
      splitPane(child, focusedId, dir, newSessionId),
    ),
  };
}

/** Swap one leaf id for another, keeping the split tree intact. */
export function replaceLeafId(
  node: LayoutNode,
  fromId: string,
  toId: string,
): LayoutNode {
  if (fromId === toId) return node;
  if (node.type === "leaf") {
    return node.id === fromId ? leaf(toId) : node;
  }
  return {
    ...node,
    children: node.children.map((child) => replaceLeafId(child, fromId, toId)),
  };
}

/** Drop a leaf. Parent splits collapse to the remaining child. */
export function removePane(
  node: LayoutNode,
  sessionId: string,
): LayoutNode | null {
  if (node.type === "leaf") return node.id === sessionId ? null : node;

  const kept: { child: LayoutNode; size: number }[] = [];
  for (let i = 0; i < node.children.length; i++) {
    const child = removePane(node.children[i], sessionId);
    if (child) kept.push({ child, size: node.sizes[i] ?? 0 });
  }
  if (kept.length === 0) return null;
  if (kept.length === 1) return kept[0].child;
  return {
    ...node,
    children: kept.map((item) => item.child),
    sizes: normalize(kept.map((item) => item.size)),
  };
}

/**
 * Close one pane in a tab. Remaining chats, files, and terminals stay;
 * returns null only when this was the last leaf.
 */
export function closeLeaf(
  tab: WorkspaceTab,
  leafId: string,
): WorkspaceTab | null {
  const nextLayout = removePane(tab.layout, leafId);
  if (!nextLayout) return null;
  const nextFocus =
    tab.focusedId === leafId
      ? (siblingLeafId(tab.layout, leafId) ?? firstLeafId(nextLayout))
      : tab.focusedId;
  return { ...tab, layout: nextLayout, focusedId: nextFocus };
}

/** Move the sash between `index` and `index + 1` to `boundary` (0–1 of the group). */
export function setSplitRatio(
  node: LayoutNode,
  splitId: string,
  index: number,
  boundary: number,
): LayoutNode {
  if (node.type === "leaf") return node;
  if (node.id !== splitId) {
    return {
      ...node,
      children: node.children.map((child) =>
        setSplitRatio(child, splitId, index, boundary),
      ),
    };
  }
  if (index < 0 || index >= node.sizes.length - 1) return node;

  return { ...node, sizes: splitSizesAtBoundary(node.sizes, index, boundary) };
}

export function splitSizesAtBoundary(
  current: number[],
  index: number,
  boundary: number,
): number[] {
  if (index < 0 || index >= current.length - 1) return current;
  const sizes = [...current];
  const before = sizes.slice(0, index).reduce((sum, n) => sum + n, 0);
  const pair = sizes[index] + sizes[index + 1];
  const min = Math.min(MIN_SIZE, pair / 2);
  const first = Math.min(pair - min, Math.max(min, boundary - before));
  sizes[index] = first;
  sizes[index + 1] = pair - first;
  return sizes;
}

export function leafIds(node: LayoutNode): string[] {
  if (node.type === "leaf") return [node.id];
  return node.children.flatMap(leafIds);
}

export function firstLeafId(node: LayoutNode): string {
  return node.type === "leaf" ? node.id : firstLeafId(node.children[0]);
}

export type LayoutRect = { x: number; y: number; w: number; h: number };

export type LayoutLeaf = {
  id: string;
  rect: LayoutRect;
  axis: "x" | "y";
};

export type LayoutSash = {
  splitId: string;
  index: number;
  dir: SplitDir;
  group: LayoutRect;
  sizes: number[];
};

export function layoutLeaves(
  node: LayoutNode,
  rect: LayoutRect = { x: 0, y: 0, w: 1, h: 1 },
  parentDir?: SplitDir,
): LayoutLeaf[] {
  const axis = parentDir === "down" ? "y" : "x";
  if (node.type === "leaf") return [{ id: node.id, rect, axis }];
  const row = node.dir === "right";
  let offset = 0;
  const out: LayoutLeaf[] = [];
  for (let i = 0; i < node.children.length; i++) {
    const size = node.sizes[i] ?? 0;
    const child: LayoutRect = row
      ? { x: rect.x + offset * rect.w, y: rect.y, w: size * rect.w, h: rect.h }
      : { x: rect.x, y: rect.y + offset * rect.h, w: rect.w, h: size * rect.h };
    offset += size;
    out.push(...layoutLeaves(node.children[i], child, node.dir));
  }
  return out;
}

export function layoutSashes(
  node: LayoutNode,
  rect: LayoutRect = { x: 0, y: 0, w: 1, h: 1 },
): LayoutSash[] {
  if (node.type === "leaf") return [];
  const row = node.dir === "right";
  let offset = 0;
  const out: LayoutSash[] = [];
  for (let i = 0; i < node.children.length; i++) {
    const size = node.sizes[i] ?? 0;
    if (i > 0) {
      out.push({
        splitId: node.id,
        index: i - 1,
        dir: node.dir,
        group: rect,
        sizes: node.sizes,
      });
    }
    const child: LayoutRect = row
      ? { x: rect.x + offset * rect.w, y: rect.y, w: size * rect.w, h: rect.h }
      : { x: rect.x, y: rect.y + offset * rect.h, w: rect.w, h: size * rect.h };
    offset += size;
    out.push(...layoutSashes(node.children[i], child));
  }
  return out;
}

function leafRects(node: LayoutNode): LayoutLeaf[] {
  return layoutLeaves(node);
}

function rangeOverlap(a0: number, a1: number, b0: number, b1: number): number {
  return Math.max(0, Math.min(a1, b1) - Math.max(a0, b0));
}

/** Adjacent leaf in `dir`, preferring panes that share an edge. */
export function neighborLeafId(
  node: LayoutNode,
  focusedId: string,
  dir: FocusDir,
): string | null {
  const panes = leafRects(node);
  const current = panes.find((p) => p.id === focusedId);
  if (!current) return null;
  const c = current.rect;

  let best: { id: string; hit: number; gap: number; overlap: number } | null =
    null;
  for (const pane of panes) {
    if (pane.id === focusedId) continue;
    const r = pane.rect;
    let gap = Infinity;
    let perp = 0;
    if (dir === "left" && r.x + r.w <= c.x + 1e-6) {
      gap = c.x - (r.x + r.w);
      perp = rangeOverlap(c.y, c.y + c.h, r.y, r.y + r.h);
    } else if (dir === "right" && r.x >= c.x + c.w - 1e-6) {
      gap = r.x - (c.x + c.w);
      perp = rangeOverlap(c.y, c.y + c.h, r.y, r.y + r.h);
    } else if (dir === "up" && r.y + r.h <= c.y + 1e-6) {
      gap = c.y - (r.y + r.h);
      perp = rangeOverlap(c.x, c.x + c.w, r.x, r.x + r.w);
    } else if (dir === "down" && r.y >= c.y + c.h - 1e-6) {
      gap = r.y - (c.y + c.h);
      perp = rangeOverlap(c.x, c.x + c.w, r.x, r.x + r.w);
    } else {
      continue;
    }
    const hit = perp > 0 ? 0 : 1;
    if (
      !best ||
      hit < best.hit ||
      (hit === best.hit && gap < best.gap) ||
      (hit === best.hit && gap === best.gap && perp > best.overlap)
    ) {
      best = { id: pane.id, hit, gap, overlap: perp };
    }
  }
  return best?.id ?? null;
}

/** Session to focus after closing `sessionId` — a neighbor's first leaf. */
export function siblingLeafId(
  node: LayoutNode,
  sessionId: string,
): string | null {
  if (node.type === "leaf") return null;
  const index = node.children.findIndex(
    (child) => child.type === "leaf" && child.id === sessionId,
  );
  if (index >= 0) {
    const neighbor = node.children[index - 1] ?? node.children[index + 1];
    return neighbor ? firstLeafId(neighbor) : null;
  }
  for (const child of node.children) {
    const found = siblingLeafId(child, sessionId);
    if (found) return found;
  }
  return null;
}

export type PanePlace = "before" | "after";
export type PaneEdge = "left" | "right" | "top" | "bottom";

type SplitNode = Extract<LayoutNode, { type: "split" }>;

export function paneEdgeFromPoint(
  x: number,
  y: number,
  rect: { left: number; top: number; width: number; height: number },
): PaneEdge {
  const nx = rect.width <= 0 ? 0 : (x - rect.left) / rect.width - 0.5;
  const ny = rect.height <= 0 ? 0 : (y - rect.top) / rect.height - 0.5;
  if (Math.abs(nx) > Math.abs(ny)) return nx < 0 ? "left" : "right";
  return ny < 0 ? "top" : "bottom";
}

function edgeSplit(edge: PaneEdge): { dir: SplitDir; place: PanePlace } {
  if (edge === "left") return { dir: "right", place: "before" };
  if (edge === "right") return { dir: "right", place: "after" };
  if (edge === "top") return { dir: "down", place: "before" };
  return { dir: "down", place: "after" };
}

function leafParent(
  node: LayoutNode,
  leafId: string,
): { parentId: string; index: number; dir: SplitDir } | null {
  if (node.type === "leaf") return null;
  for (let i = 0; i < node.children.length; i++) {
    const child = node.children[i];
    if (child.type === "leaf" && child.id === leafId) {
      return { parentId: node.id, index: i, dir: node.dir };
    }
    const found = leafParent(child, leafId);
    if (found) return found;
  }
  return null;
}

function reorderChild(
  split: SplitNode,
  fromIndex: number,
  toIndex: number,
  place: PanePlace,
): SplitNode {
  const n = split.children.length;
  if (fromIndex < 0 || fromIndex >= n || toIndex < 0 || toIndex >= n) {
    return split;
  }
  let insertAt = place === "after" ? toIndex + 1 : toIndex;
  insertAt = Math.max(0, Math.min(n, insertAt));
  if (fromIndex < insertAt) insertAt -= 1;
  if (fromIndex === insertAt) return split;
  const children = [...split.children];
  const sizes = [...split.sizes];
  const [child] = children.splice(fromIndex, 1);
  const [size] = sizes.splice(fromIndex, 1);
  children.splice(insertAt, 0, child);
  sizes.splice(insertAt, 0, size);
  return { ...split, children, sizes };
}

function reorderInSplit(
  node: LayoutNode,
  splitId: string,
  fromIndex: number,
  toIndex: number,
  place: PanePlace,
): LayoutNode {
  if (node.type === "leaf") return node;
  if (node.id === splitId) return reorderChild(node, fromIndex, toIndex, place);
  return {
    ...node,
    children: node.children.map((child) =>
      reorderInSplit(child, splitId, fromIndex, toIndex, place),
    ),
  };
}

function extractLeaf(
  node: LayoutNode,
  leafId: string,
): {
  tree: LayoutNode | null;
  leaf: Extract<LayoutNode, { type: "leaf" }>;
} | null {
  if (node.type === "leaf") {
    return node.id === leafId ? { tree: null, leaf: node } : null;
  }

  const children: LayoutNode[] = [];
  const sizes: number[] = [];
  let leaf: Extract<LayoutNode, { type: "leaf" }> | null = null;

  for (let i = 0; i < node.children.length; i++) {
    const extracted = extractLeaf(node.children[i], leafId);
    if (!extracted) {
      children.push(node.children[i]);
      sizes.push(node.sizes[i] ?? 0);
      continue;
    }
    leaf = extracted.leaf;
    if (extracted.tree) {
      children.push(extracted.tree);
      sizes.push(node.sizes[i] ?? 0);
    }
  }

  if (!leaf) return null;
  if (children.length === 0) return { tree: null, leaf };
  if (children.length === 1) return { tree: children[0], leaf };
  return {
    tree: { ...node, children, sizes: normalize(sizes) },
    leaf,
  };
}

function insertBeside(
  node: LayoutNode,
  targetId: string,
  leaf: LayoutNode,
  place: PanePlace,
): LayoutNode {
  if (node.type === "leaf") return node;

  const index = node.children.findIndex(
    (child) => child.type === "leaf" && child.id === targetId,
  );
  if (index >= 0) {
    const insertAt = place === "before" ? index : index + 1;
    const children = [...node.children];
    const sizes = [...node.sizes];
    const share = (sizes[index] ?? 0) / 2;
    sizes[index] = share;
    children.splice(insertAt, 0, leaf);
    sizes.splice(insertAt, 0, share);
    return { ...node, children, sizes };
  }

  return {
    ...node,
    children: node.children.map((child) =>
      insertBeside(child, targetId, leaf, place),
    ),
  };
}

function wrapBeside(
  node: LayoutNode,
  targetId: string,
  incoming: LayoutNode,
  dir: SplitDir,
  place: PanePlace,
): LayoutNode {
  if (node.type === "leaf") {
    if (node.id !== targetId) return node;
    return {
      type: "split",
      id: crypto.randomUUID(),
      dir,
      children: place === "before" ? [incoming, node] : [node, incoming],
      sizes: [0.5, 0.5],
    };
  }
  return {
    ...node,
    children: node.children.map((child) =>
      wrapBeside(child, targetId, incoming, dir, place),
    ),
  };
}

/**
 * Drag a leaf onto another pane's edge. Same-axis siblings keep their
 * sizes and only swap order; a perpendicular edge nests a new split
 * around the target; dropping onto a pane in another group of the
 * same axis relocates the leaf there.
 */
export function movePane(
  node: LayoutNode,
  fromId: string,
  toId: string,
  edge: PaneEdge,
): LayoutNode {
  if (fromId === toId) return node;
  const fromAt = leafParent(node, fromId);
  const toAt = leafParent(node, toId);
  if (!fromAt || !toAt) return node;

  const { dir, place } = edgeSplit(edge);
  if (toAt.dir === dir && fromAt.parentId === toAt.parentId) {
    return reorderInSplit(
      node,
      fromAt.parentId,
      fromAt.index,
      toAt.index,
      place,
    );
  }

  const extracted = extractLeaf(node, fromId);
  if (!extracted?.tree) return node;
  if (!leafIds(extracted.tree).includes(toId)) return node;
  const targetAt = leafParent(extracted.tree, toId);
  if (targetAt?.dir === dir) {
    return insertBeside(extracted.tree, toId, extracted.leaf, place);
  }
  return wrapBeside(extracted.tree, toId, extracted.leaf, dir, place);
}

/**
 * Open `sessionId` on `toId`'s edge, or move it there when it is
 * already a leaf in this tree.
 */
export function placePane(
  node: LayoutNode,
  sessionId: string,
  toId: string,
  edge: PaneEdge,
): LayoutNode {
  if (sessionId === toId) return node;
  const ids = leafIds(node);
  if (!ids.includes(toId)) return node;
  if (ids.includes(sessionId)) return movePane(node, sessionId, toId, edge);

  const { dir, place } = edgeSplit(edge);
  const incoming = leaf(sessionId);
  const targetAt = leafParent(node, toId);
  if (targetAt?.dir === dir) {
    return insertBeside(node, toId, incoming, place);
  }
  return wrapBeside(node, toId, incoming, dir, place);
}
