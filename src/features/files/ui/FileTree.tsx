import {
  ChevronDown,
  ChevronRight,
  FilePlus,
  FolderPlus,
  FoldVertical,
  Search,
} from "../../../shared/ui/icons";
import {
  createContext,
  memo,
  useContext,
  useEffect,
  useRef,
  useState,
  useSyncExternalStore,
  type KeyboardEvent as ReactKeyboardEvent,
  type MouseEvent as ReactMouseEvent,
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
} from "react";
import {
  leafName,
  validateFileName,
  wellFormedFileName,
  type NameIssue,
} from "../model/fileName";
import { useLockOverscroll } from "../../../shared/hooks/useLockOverscroll";
import {
  loadShowExcludedFiles,
  subscribeShowExcludedFiles,
} from "../../settings/model/appearance";
import {
  bulkError,
  createParentOf,
  dirsTouchedByCreate,
  dirsTouchedByMove,
  forgetDir,
  listCachedDir,
  loadExpanded,
  loadSelected,
  notifyDirsChanged,
  peekDir,
  refreshDir,
  saveExpanded,
  saveSelected,
  subscribeDirsChanged,
  visibleChildren,
  visibleTreeOrder,
} from "../model/fileTree";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { REMOTE_PATH_PREFIX } from "../../../shared/lib/remotePaths";
import { dragPointToClient } from "../../../shared/lib/dragPoint";
import {
  basename,
  clipboardFilePaths,
  copyPath,
  createPath,
  deletePath,
  movePath,
  renamePath,
  revealPath,
  type FsEntry,
} from "../../../platform/tauri/fs";
import {
  displayPath,
  isEqualOrInside,
  parentPath,
  rebasePath,
  topLevelPaths,
} from "../../../shared/lib/paths";
import { runSequential } from "../../../shared/lib/concurrent";
import {
  selectionMode,
  type SelectMode,
} from "../../../shared/lib/multiSelection";
import { useMultiSelection } from "../../../shared/hooks/useMultiSelection";
import { IS_MAC, IS_WIN, MOD, SHIFT } from "../../../platform/tauri/platform";
import type { OpenFileFn } from "../../search/model/search";
import type { GitStatusMap } from "../../source-control/hooks/useGitFileStatuses";
import {
  emitExplorerFilePointerDrag,
  setGrabbing,
  suppressTextSelection,
} from "../../../shared/lib/drag";
import { ExplorerMenu, type ExplorerMenuItem } from "./ExplorerMenu";
import { FileTypeIcon } from "./FileTypeIcon";

const GIT_STATUS_COLOR: Record<string, string> = {
  modified: "text-amber-400",
  added: "text-emerald-400",
  untracked: "text-emerald-400",
  deleted: "text-red-400",
};

type Props = {
  cwd: string;
  /** Display identity for the root when it differs from the physical folder. */
  rootLabel?: string;
  onOpenFile: OpenFileFn;
  onOpenTerminal?: (cwd: string) => void;
  onFileMoved?: (from: string, to: string) => void;
  onFileDeleted?: (path: string) => void;
  onSearch?: () => void;
  gitStatuses?: GitStatusMap;
};

type Creating = { id: number; parent: string; isDir: boolean };
type Clip = { mode: "copy" | "cut"; paths: string[] };
/** `paths` are the rows the action applies to: the selection when `path` is in it. */
type MenuTarget = {
  path: string;
  paths: string[];
  isDir: boolean;
  isRoot: boolean;
};
type MenuState = { x: number; y: number; target: MenuTarget };

const REVEAL_LABEL = IS_MAC
  ? "Reveal in Finder"
  : IS_WIN
    ? "Reveal in File Explorer"
    : "Open Containing Folder";

type TreeCtxValue = {
  expanded: Set<string>;
  selectedPath: string | null;
  creating: Creating | null;
  renaming: string | null;
  cutPaths: readonly string[];
  /** Whether the multi-selection holds any row; the focus is only highlighted when not. */
  hasSelection: boolean;
  isSelected: (path: string) => boolean;
  onRowClick: (event: ReactMouseEvent, path: string) => SelectMode;
  dragOverPath: string | null;
  epoch: number;
  showExcludedFiles: boolean;
  gitStatuses?: GitStatusMap;
  onToggle: (path: string) => void;
  onSelect: (path: string) => void;
  onFilePointerDown: (
    path: string,
    event: ReactPointerEvent<HTMLButtonElement>,
  ) => void;
  consumeFileClick: () => boolean;
  onOpenFile: OpenFileFn;
  onCreateCommit: (id: number, raw: string) => Promise<void>;
  onCreateCancel: (id: number) => void;
  onRenameCommit: (path: string, raw: string) => Promise<void>;
  onRenameCancel: () => void;
  onItemContextMenu: (
    entry: { path: string; isDir: boolean },
    e: ReactMouseEvent,
  ) => void;
};

const TreeCtx = createContext<TreeCtxValue | null>(null);

function useTree(): TreeCtxValue {
  const ctx = useContext(TreeCtx);
  if (!ctx) throw new Error("TreeCtx missing");
  return ctx;
}

function isDirAt(cwd: string, path: string): boolean {
  if (path === cwd) return true;
  return (
    peekDir(parentPath(path))?.find((entry) => entry.path === path)?.isDir ??
    peekDir(path) != null
  );
}

async function copyText(text: string) {
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    const el = document.createElement("textarea");
    el.value = text;
    el.style.position = "fixed";
    el.style.left = "-9999px";
    document.body.appendChild(el);
    el.select();
    document.execCommand("copy");
    el.remove();
  }
}

/** Non-Latin layouts put the local letter in `key`, so fall back to the physical key. */
function shortcutLetter(e: ReactKeyboardEvent): string {
  const key = e.key.toLowerCase();
  if (/^[a-z]$/.test(key)) return key;
  return /^Key[A-Z]$/.test(e.code) ? e.code.slice(3).toLowerCase() : key;
}

function explorerItems(
  target: MenuTarget,
  clip: Clip | null,
  canOpenTerminal: boolean,
): ExplorerMenuItem[] {
  if (target.paths.length > 1) {
    return [
      { kind: "item", id: "cut", label: "Cut", shortcut: `${MOD}X` },
      { kind: "item", id: "copy", label: "Copy", shortcut: `${MOD}C` },
      { kind: "sep" },
      {
        kind: "item",
        id: "copy-path",
        label: "Copy Path",
        shortcut: `${MOD}${SHIFT}C`,
      },
      { kind: "item", id: "copy-relative-path", label: "Copy Relative Path" },
      { kind: "sep" },
      {
        kind: "item",
        id: "delete",
        label: "Delete",
        shortcut: "⌫",
        danger: true,
      },
    ];
  }
  const pasteParent = target.isDir ? target.path : parentPath(target.path);
  const pasteBlocked = !!clip?.paths.some((path) =>
    isEqualOrInside(pasteParent, path),
  );
  return [
    { kind: "item", id: "new-file", label: "New File" },
    { kind: "item", id: "new-folder", label: "New Folder" },
    { kind: "sep" },
    {
      kind: "item",
      id: "cut",
      label: "Cut",
      shortcut: `${MOD}X`,
      disabled: target.isRoot,
    },
    {
      kind: "item",
      id: "copy",
      label: "Copy",
      shortcut: `${MOD}C`,
      disabled: target.isRoot,
    },
    {
      kind: "item",
      id: "paste",
      label: "Paste",
      shortcut: `${MOD}V`,
      disabled: pasteBlocked,
    },
    {
      kind: "item",
      id: "duplicate",
      label: "Duplicate",
      disabled: target.isRoot,
    },
    { kind: "sep" },
    {
      kind: "item",
      id: "copy-path",
      label: "Copy Path",
      shortcut: `${MOD}${SHIFT}C`,
    },
    { kind: "item", id: "copy-relative-path", label: "Copy Relative Path" },
    { kind: "sep" },
    {
      kind: "item",
      id: "rename",
      label: "Rename",
      shortcut: "F2",
      disabled: target.isRoot,
    },
    {
      kind: "item",
      id: "delete",
      label: "Delete",
      shortcut: "⌫",
      disabled: target.isRoot,
      danger: true,
    },
    { kind: "sep" },
    ...(canOpenTerminal
      ? [
          {
            kind: "item" as const,
            id: "open-terminal",
            label: "Open in Terminal",
          },
        ]
      : []),
    { kind: "item", id: "reveal", label: REVEAL_LABEL },
  ];
}

// Chat updates rerender the sidebar even when Files is hidden. Keep its tree
// intact unless file-tree props, local state, or subscriptions actually change.
export const FileTree = memo(function FileTree({
  cwd,
  rootLabel,
  onOpenFile,
  onOpenTerminal,
  onFileMoved,
  onFileDeleted,
  onSearch,
  gitStatuses,
}: Props) {
  const [expanded, setExpanded] = useState(() => loadExpanded(cwd));
  const [selectedPath, setSelectedPath] = useState(() => loadSelected(cwd));
  const [children, setChildren] = useState<FsEntry[] | null>(() =>
    peekDir(cwd),
  );
  const [error, setError] = useState<string | null>(null);
  const [creating, setCreating] = useState<Creating | null>(null);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [clip, setClip] = useState<Clip | null>(null);
  const [menu, setMenu] = useState<MenuState | null>(null);
  const [dragOverPath, setDragOverPath] = useState<string | null>(null);
  const [opError, setOpError] = useState<string | null>(null);
  const [epoch, setEpoch] = useState(0);
  const showExcludedFiles = useSyncExternalStore(
    subscribeShowExcludedFiles,
    loadShowExcludedFiles,
    loadShowExcludedFiles,
  );
  // Rows picked with Cmd/Ctrl/Shift-click; `selectedPath` stays the focus
  // (shortcut anchor, rename, create parent) and is the only persisted part.
  const sel = useMultiSelection({
    visibleOrder: () => visibleTreeOrder(cwd, expanded, showExcludedFiles),
    fallbackAnchor: () => (selectedPath !== cwd ? selectedPath : null),
  });
  const creatingRef = useRef(creating);
  creatingRef.current = creating;
  const fileDragCleanup = useRef<(() => void) | null>(null);
  const suppressFileClickUntil = useRef(0);
  const rootRef = useRef<HTMLDivElement>(null);
  const lockOverscroll = useLockOverscroll<HTMLDivElement>();
  const name = rootLabel?.trim() || basename(cwd);
  const rootOpen = expanded.has(cwd);

  const toggle = (path: string) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      saveExpanded(cwd, next);
      return next;
    });
  };

  const onSelect = (path: string) => {
    setSelectedPath(path);
    saveSelected(cwd, path);
  };

  /** Focuses a row the tree just produced (created, pasted…), dropping the multi-selection. */
  const focusPath = (path: string) => {
    onSelect(path);
    sel.clear();
  };

  /** Rows a shortcut acts on: the selection, else the focus. */
  const keyTargets = (focus: string) =>
    sel.selection.ids.length ? topLevelPaths(sel.selection.ids) : [focus];

  const onFilePointerDown = (
    path: string,
    event: ReactPointerEvent<HTMLButtonElement>,
  ) => {
    if (event.button !== 0 || fileDragCleanup.current) return;
    const handle = event.currentTarget;
    const pointerId = event.pointerId;
    const startX = event.clientX;
    const startY = event.clientY;
    let lastX = startX;
    let lastY = startY;
    let active = false;
    // Set on activation: the grabbed file, or every selected file with it.
    let paths = [path];
    let restoreSelection: (() => void) | undefined;
    let preview: HTMLDivElement | null = null;

    const movePreview = () => {
      if (!preview) return;
      const edge = 8;
      const grabX = 12;
      const grabY = 13;
      const width = preview.offsetWidth;
      const height = preview.offsetHeight;
      const x = Math.min(
        Math.max(edge, lastX - grabX),
        Math.max(edge, window.innerWidth - width - edge),
      );
      const y = Math.min(
        Math.max(edge, lastY - grabY),
        Math.max(edge, window.innerHeight - height - edge),
      );
      preview.style.transform = `translate3d(${Math.round(x)}px, ${Math.round(
        y,
      )}px, 0)`;
    };

    const createPreview = () => {
      preview = document.createElement("div");
      preview.setAttribute("aria-hidden", "true");
      preview.classList.add("explorer-file-drag-preview");

      // Keep the useful identity of the row without dragging its full-width
      // layout, indentation spacer, selection state, or button behavior.
      const icon = handle.children.item(1)?.cloneNode(true);
      const label = handle.children.item(2)?.cloneNode(true);
      if (icon) preview.append(icon);
      if (label) preview.append(label);
      if (paths.length > 1) {
        const count = document.createElement("span");
        count.classList.add("explorer-file-drag-count");
        count.textContent = `+${paths.length - 1}`;
        preview.append(count);
      }

      document.body.append(preview);
      movePreview();
    };

    const release = () => {
      delete handle.dataset.explorerDragging;
      preview?.remove();
      preview = null;
      document.documentElement.classList.remove("is-explorer-file-dragging");
      if (restoreSelection) {
        restoreSelection();
        restoreSelection = undefined;
        setGrabbing(false);
      }
      try {
        if (handle.hasPointerCapture(pointerId))
          handle.releasePointerCapture(pointerId);
      } catch {
        /* already released */
      }
    };

    const reset = () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      window.removeEventListener("pointercancel", onCancel);
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("blur", onCancel);
      release();
      if (active) emitExplorerFilePointerDrag({ type: "end", paths });
      fileDragCleanup.current = null;
    };

    const activate = () => {
      active = true;
      // Dragging a row outside the selection drags that row alone, as Finder.
      if (!sel.isSelected(path)) sel.select(path, undefined, "single");
      // Folders aren't draggable: drop them first, so files selected inside a
      // selected folder still come along.
      paths = topLevelPaths(
        sel.targetsFor(path).filter((p) => !isDirAt(cwd, p)),
      );
      onSelect(path);
      restoreSelection = suppressTextSelection();
      setGrabbing(true);
      createPreview();
      document.documentElement.classList.add("is-explorer-file-dragging");
      handle.dataset.explorerDragging = "true";
      try {
        handle.setPointerCapture(pointerId);
      } catch {
        /* window listeners still track the gesture */
      }
    };

    function onMove(moveEvent: PointerEvent) {
      if (moveEvent.pointerId !== pointerId) return;
      lastX = moveEvent.clientX;
      lastY = moveEvent.clientY;
      if (!active) {
        if (Math.hypot(lastX - startX, lastY - startY) < 5) return;
        activate();
      }
      moveEvent.preventDefault();
      movePreview();
      emitExplorerFilePointerDrag({ type: "move", paths, x: lastX, y: lastY });
    }

    function finish(commit: boolean, upEvent?: PointerEvent) {
      if (commit && upEvent) onMove(upEvent);
      if (active) {
        suppressFileClickUntil.current = performance.now() + 400;
        if (commit) {
          emitExplorerFilePointerDrag({
            type: "drop",
            paths,
            x: lastX,
            y: lastY,
          });
        }
      }
      reset();
    }

    function onUp(upEvent: PointerEvent) {
      if (upEvent.pointerId === pointerId) finish(true, upEvent);
    }
    function onCancel() {
      finish(false);
    }
    function onKey(keyEvent: KeyboardEvent) {
      if (keyEvent.key !== "Escape") return;
      keyEvent.preventDefault();
      finish(false);
    }

    fileDragCleanup.current = onCancel;
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
    window.addEventListener("pointercancel", onCancel);
    window.addEventListener("keydown", onKey);
    window.addEventListener("blur", onCancel);
  };

  const consumeFileClick = () =>
    performance.now() < suppressFileClickUntil.current;

  useEffect(() => () => fileDragCleanup.current?.(), []);

  const expandDirs = (dirs: string[]) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      for (const dir of dirs) next.add(dir);
      saveExpanded(cwd, next);
      return next;
    });
  };

  const refreshTouched = async (touched: string[], forget: string[] = []) => {
    for (const path of forget) forgetDir(path);
    await Promise.all([...new Set(touched)].map((path) => refreshDir(path)));
    setEpoch((n) => n + 1);
  };

  const remapTreePaths = (from: string, to: string) => {
    setExpanded((prev) => {
      const next = new Set<string>();
      for (const path of prev) next.add(rebasePath(path, from, to));
      saveExpanded(cwd, next);
      return next;
    });
    setSelectedPath((prev) => {
      const next = prev ? rebasePath(prev, from, to) : prev;
      saveSelected(cwd, next);
      return next;
    });
    setClip((cur) =>
      cur
        ? { ...cur, paths: cur.paths.map((p) => rebasePath(p, from, to)) }
        : cur,
    );
  };

  const startCreate = (
    isDir: boolean,
    atPath: string | null = selectedPath,
  ) => {
    const parent = createParentOf(cwd, atPath);
    setRenaming(null);
    expandDirs([cwd, parent]);
    setCreating({ id: Date.now(), parent, isDir });
  };

  const startRename = (path: string) => {
    if (path === cwd) return;
    setCreating(null);
    setMenu(null);
    onSelect(path);
    setRenaming(path);
  };

  const onCreateCancel = (id: number) => {
    setCreating((cur) => (cur?.id === id ? null : cur));
  };

  const onCreateCommit = async (id: number, raw: string) => {
    const session = creatingRef.current;
    if (!session || session.id !== id) return;
    const asFolder = session.isDir || /[/\\]$/.test(raw);
    const fileName = wellFormedFileName(raw);
    const created = await createPath(session.parent, fileName, asFolder);
    const touched = dirsTouchedByCreate(session.parent, fileName);
    await refreshTouched(touched);
    setCreating((cur) => (cur?.id === id ? null : cur));
    expandDirs(touched);
    focusPath(created);
    if (!asFolder) onOpenFile(created, undefined, { exact: true });
  };

  const onRenameCancel = () => setRenaming(null);

  const onRenameCommit = async (path: string, raw: string) => {
    const fileName = wellFormedFileName(raw);
    if (!fileName || (fileName === basename(path) && !/[/\\]/.test(raw))) {
      setRenaming(null);
      return;
    }
    const next = await renamePath(path, fileName);
    const wasDir = isDirAt(cwd, path);
    const parent = parentPath(path);
    await refreshTouched(
      [...dirsTouchedByCreate(parent, fileName), parent],
      wasDir ? [path] : [],
    );
    setRenaming(null);
    expandDirs(dirsTouchedByCreate(parent, fileName));
    remapTreePaths(path, next);
    onFileMoved?.(path, next);
  };

  const removeEntries = async (paths: string[]) => {
    const targets = paths.filter((path) => path !== cwd);
    if (!targets.length) return;
    const dirs = new Set(targets.filter((path) => isDirAt(cwd, path)));
    const label = basename(targets[0]);
    const ok = window.confirm(
      targets.length > 1
        ? `Delete ${targets.length} items?`
        : dirs.size
          ? `Delete folder “${label}” and everything inside it?`
          : `Delete “${label}”?`,
    );
    if (!ok) return;
    const { done, failed } = await runSequential(targets, deletePath);
    if (done.length) {
      await refreshTouched(
        done.map(parentPath),
        done.filter((path) => dirs.has(path)),
      );
      setSelectedPath((prev) => {
        const gone = prev
          ? done.find((path) => isEqualOrInside(prev, path))
          : done[0];
        if (!gone) return prev;
        const parent = parentPath(gone);
        saveSelected(cwd, parent);
        return parent;
      });
      setClip((cur) => {
        const kept = cur?.paths.filter(
          (p) => !done.some((path) => isEqualOrInside(p, path)),
        );
        return cur && kept?.length ? { ...cur, paths: kept } : null;
      });
      sel.clear();
      for (const path of done) onFileDeleted?.(path);
    }
    const error = bulkError(failed);
    if (error) throw error;
  };

  const copyExternalFiles = async (paths: string[], destParent: string) => {
    let created: string | null = null;
    const { failed } = await runSequential(paths, async (from) => {
      created = await copyPath(from, destParent);
    });
    if (created) {
      await refreshTouched([destParent]);
      expandDirs([destParent]);
      focusPath(created);
    }
    const error = bulkError(failed);
    if (error) throw error;
  };

  const pasteAt = async (targetPath: string) => {
    const destParent = createParentOf(cwd, targetPath);
    if (!clip) {
      await copyExternalFiles(await clipboardFilePaths(), destParent);
      return;
    }
    if (clip.paths.some((path) => isEqualOrInside(destParent, path))) {
      throw new Error("Cannot paste a folder into itself.");
    }
    const { mode, paths } = clip;
    const dirs = new Set(paths.filter((path) => isDirAt(cwd, path)));
    const created = new Map<string, string>();
    const { failed } = await runSequential(paths, async (from) => {
      created.set(
        from,
        mode === "cut"
          ? await movePath(from, destParent)
          : await copyPath(from, destParent),
      );
    });
    if (created.size) {
      const moves = [...created];
      if (mode === "cut") {
        await refreshTouched(
          moves.flatMap(([from, to]) => dirsTouchedByMove(from, to)),
          moves.filter(([from]) => dirs.has(from)).map(([from]) => from),
        );
        for (const [from, to] of moves) {
          remapTreePaths(from, to);
          onFileMoved?.(from, to);
        }
        // Keep what failed to move on the clipboard so Paste can retry it.
        const left = paths.filter((path) => !created.has(path));
        setClip(left.length ? { mode, paths: left } : null);
      } else {
        await refreshTouched([destParent]);
      }
      expandDirs([destParent]);
      focusPath(moves[moves.length - 1][1]);
    }
    const error = bulkError(failed);
    if (error) throw error;
  };

  const duplicateAt = async (path: string) => {
    if (path === cwd) return;
    const destParent = parentPath(path);
    const created = await copyPath(path, destParent);
    await refreshTouched([destParent]);
    focusPath(created);
  };

  const run = async (work: () => Promise<void>) => {
    setOpError(null);
    try {
      await work();
    } catch (err: unknown) {
      setOpError(err instanceof Error ? err.message : String(err));
    }
  };

  const dropFiles = (paths: string[], targetPath: string) =>
    run(() => copyExternalFiles(paths, createParentOf(cwd, targetPath)));
  const dropFilesRef = useRef(dropFiles);
  dropFilesRef.current = dropFiles;

  const openMenu = (target: MenuTarget, x: number, y: number) => {
    setCreating(null);
    setRenaming(null);
    onSelect(target.path);
    setMenu({ x, y, target });
  };

  const runAction = async (id: string, target: MenuTarget) => {
    switch (id) {
      case "new-file":
        startCreate(false, target.path);
        return;
      case "new-folder":
        startCreate(true, target.path);
        return;
      case "cut":
        if (target.isRoot) return;
        setClip({ mode: "cut", paths: target.paths });
        return;
      case "copy":
        if (target.isRoot) return;
        setClip({ mode: "copy", paths: target.paths });
        return;
      case "paste":
        await run(() => pasteAt(target.path));
        return;
      case "duplicate":
        await run(() => duplicateAt(target.path));
        return;
      case "copy-path":
        await copyText(target.paths.join("\n"));
        return;
      case "copy-relative-path":
        await copyText(
          target.paths.map((path) => displayPath(path, cwd)).join("\n"),
        );
        return;
      case "rename":
        startRename(target.path);
        return;
      case "delete":
        await run(() => removeEntries(target.paths));
        return;
      case "reveal":
        await run(() => revealPath(target.path));
        return;
      case "open-terminal":
        onOpenTerminal?.(target.isDir ? target.path : parentPath(target.path));
        return;
    }
  };

  const onItemContextMenu = (
    entry: { path: string; isDir: boolean },
    e: ReactMouseEvent,
  ) => {
    e.preventDefault();
    e.stopPropagation();
    // A row outside the selection replaces it; one inside keeps it.
    if (!sel.isSelected(entry.path)) {
      sel.select(entry.path, undefined, "single");
    }
    openMenu(
      {
        path: entry.path,
        paths: topLevelPaths(sel.targetsFor(entry.path)),
        isDir: entry.isDir,
        isRoot: false,
      },
      e.clientX,
      e.clientY,
    );
  };

  const openRootMenu = (e: ReactMouseEvent) => {
    sel.clear();
    openMenu(
      { path: cwd, paths: [cwd], isDir: true, isRoot: true },
      e.clientX,
      e.clientY,
    );
  };

  const onBackgroundMenu = (e: ReactMouseEvent) => {
    if ((e.target as HTMLElement).closest("input")) return;
    e.preventDefault();
    openRootMenu(e);
  };

  const onKeyDown = (e: ReactKeyboardEvent<HTMLDivElement>) => {
    if ((e.target as HTMLElement).closest("input")) return;
    if (
      (e.target as HTMLElement).closest("button") &&
      !(e.target as HTMLElement).closest(
        "[role='treeitem'], [data-explorer-root]",
      )
    ) {
      return;
    }
    // Escape clears the selection, Mod+A selects every visible row.
    if (sel.onKeyDown(e, "")) return;
    const path = selectedPath ?? cwd;
    const targets = keyTargets(path);
    const isRoot = targets.includes(cwd);
    const mod = e.metaKey || e.ctrlKey;
    const key = shortcutLetter(e);
    if (mod && !e.altKey && e.shiftKey && key === "c") {
      e.preventDefault();
      void copyText(targets.join("\n"));
      return;
    }
    if (mod && !e.altKey && !e.shiftKey && key === "c") {
      if (isRoot) return;
      e.preventDefault();
      setClip({ mode: "copy", paths: targets });
      return;
    }
    if (mod && !e.altKey && !e.shiftKey && key === "x") {
      if (isRoot) return;
      e.preventDefault();
      setClip({ mode: "cut", paths: targets });
      return;
    }
    if (mod && !e.altKey && !e.shiftKey && key === "v") {
      e.preventDefault();
      void run(() => pasteAt(path));
      return;
    }
    if (e.key === "F2") {
      if (targets.length !== 1) return;
      e.preventDefault();
      startRename(targets[0]);
      return;
    }
    if (e.key === "Delete" || e.key === "Backspace") {
      e.preventDefault();
      void run(() => removeEntries(targets));
      return;
    }
    if (e.key === "Escape" && clip?.mode === "cut") {
      e.preventDefault();
      setClip(null);
    }
  };

  // The selection only holds visible rows: collapsing a folder, hiding
  // excluded files or a filesystem change drops the rows that went away.
  const { prune } = sel;
  useEffect(() => {
    const visible = new Set(visibleTreeOrder(cwd, expanded, showExcludedFiles));
    prune((path) => visible.has(path));
  }, [cwd, epoch, expanded, showExcludedFiles, prune]);

  useEffect(() => {
    if (!menu) return;
    const onScroll = () => setMenu(null);
    const scrollParent = rootRef.current?.closest(".overflow-y-auto") ?? window;
    scrollParent.addEventListener("scroll", onScroll, true);
    return () => scrollParent.removeEventListener("scroll", onScroll, true);
  }, [menu]);

  useEffect(() => {
    const treePathAt = (x: number, y: number): string | null => {
      const root = rootRef.current;
      if (!root) return null;
      const point = dragPointToClient(x, y);
      const el = document.elementFromPoint(point.x, point.y);
      if (!el || !root.contains(el)) return null;
      return el.closest<HTMLElement>("[role='treeitem']")?.title ?? cwd;
    };

    let cancelled = false;
    let unlisten: (() => void) | undefined;
    void getCurrentWebview()
      .onDragDropEvent((event) => {
        if (event.payload.type === "leave") {
          setDragOverPath(null);
          return;
        }
        const { x, y } = event.payload.position;
        const target = treePathAt(x, y);
        if (event.payload.type !== "drop") {
          setDragOverPath(target ? createParentOf(cwd, target) : null);
          return;
        }
        setDragOverPath(null);
        if (target) void dropFilesRef.current(event.payload.paths, target);
      })
      .then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [cwd]);

  useEffect(() => {
    const unsub = subscribeDirsChanged(() => setEpoch((n) => n + 1));
    const onResume = () => {
      if (!document.hidden) notifyDirsChanged();
    };
    window.addEventListener("focus", onResume);
    document.addEventListener("visibilitychange", onResume);
    return () => {
      unsub();
      window.removeEventListener("focus", onResume);
      document.removeEventListener("visibilitychange", onResume);
    };
  }, []);

  useEffect(() => {
    if (!cwd.startsWith(REMOTE_PATH_PREFIX)) return;
    const timer = window.setInterval(() => {
      if (!document.hidden) notifyDirsChanged();
    }, 5000);
    return () => window.clearInterval(timer);
  }, [cwd]);

  useEffect(() => {
    const hit = peekDir(cwd);
    if (hit) {
      setChildren(hit);
      setError(null);
      return;
    }
    let cancelled = false;
    setChildren(null);
    setError(null);
    void listCachedDir(cwd)
      .then((entries) => {
        if (!cancelled) setChildren(entries);
      })
      .catch((err: unknown) => {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : String(err));
          setChildren([]);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [cwd, epoch]);

  return (
    <TreeCtx.Provider
      value={{
        expanded,
        selectedPath,
        creating,
        renaming,
        cutPaths: clip?.mode === "cut" ? clip.paths : [],
        hasSelection: sel.selection.ids.length > 0,
        isSelected: sel.isSelected,
        onRowClick: sel.onRowClick,
        dragOverPath,
        epoch,
        showExcludedFiles,
        gitStatuses,
        onToggle: toggle,
        onSelect,
        onFilePointerDown,
        consumeFileClick,
        onOpenFile,
        onCreateCommit,
        onCreateCancel,
        onRenameCommit,
        onRenameCancel,
        onItemContextMenu,
      }}
    >
      <div
        ref={rootRef}
        tabIndex={-1}
        className="flex h-full min-h-0 flex-col outline-none"
        onKeyDown={onKeyDown}
        onContextMenu={onBackgroundMenu}
      >
        <div
          className="flex h-9 shrink-0 items-center gap-px overflow-visible border-b border-stroke px-2"
          onContextMenu={(e) => e.stopPropagation()}
        >
          <HeaderIcon label="New File" onClick={() => startCreate(false)}>
            <FilePlus className="size-3.5" strokeWidth={1.75} />
          </HeaderIcon>
          <HeaderIcon label="New Folder" onClick={() => startCreate(true)}>
            <FolderPlus className="size-3.5" strokeWidth={1.75} />
          </HeaderIcon>
          <HeaderIcon
            label="Collapse All"
            onClick={() => {
              setCreating(null);
              setRenaming(null);
              const next = new Set([cwd]);
              saveExpanded(cwd, next);
              setExpanded(next);
            }}
          >
            <FoldVertical className="size-3.5" strokeWidth={1.75} />
          </HeaderIcon>
          {onSearch ? (
            <HeaderIcon
              label={`Search in files (${MOD}Shift+F)`}
              onClick={onSearch}
            >
              <Search className="size-3.5" strokeWidth={1.75} />
            </HeaderIcon>
          ) : null}
        </div>
        <div className="flex h-8 shrink-0 items-center">
          <button
            type="button"
            data-explorer-root
            aria-expanded={rootOpen}
            title={cwd}
            onClick={() => {
              sel.clear();
              onSelect(cwd);
              toggle(cwd);
            }}
            onContextMenu={(e) => {
              e.preventDefault();
              e.stopPropagation();
              openRootMenu(e);
            }}
            className={`flex min-w-0 flex-1 items-center gap-1 h-full pl-2 text-left ${
              dragOverPath === cwd ? "bg-selection" : ""
            }`}
          >
            <span className="grid size-4 shrink-0 place-items-center text-content/50">
              {rootOpen ? (
                <ChevronDown className="size-3.5" strokeWidth={1.75} />
              ) : (
                <ChevronRight className="size-3.5" strokeWidth={1.75} />
              )}
            </span>
            <span className="min-w-0 truncate text-[11px] font-semibold tracking-[0.08em] text-content/50 uppercase">
              {name}
            </span>
          </button>
        </div>
        <div
          ref={lockOverscroll}
          className="min-h-0 flex-1 overflow-y-auto overscroll-none"
        >
          {opError ? (
            <p className="px-3 py-1 text-[12px] leading-4 text-red-400">
              {opError}
            </p>
          ) : null}
          {rootOpen ? (
            <div
              role="tree"
              aria-label={`${name} files`}
              aria-multiselectable="true"
            >
              <TreeChildren
                parent={cwd}
                depth={0}
                entries={children}
                loading={children === null && !error}
                error={error}
              />
            </div>
          ) : null}
        </div>
      </div>
      {menu ? (
        <ExplorerMenu
          x={menu.x}
          y={menu.y}
          items={explorerItems(menu.target, clip, !!onOpenTerminal)}
          onPick={(id) => {
            const target = menu.target;
            setMenu(null);
            void runAction(id, target);
          }}
          onClose={() => setMenu(null)}
        />
      ) : null}
    </TreeCtx.Provider>
  );
});

function HeaderIcon({
  label,
  onClick,
  active = false,
  children,
}: {
  label: string;
  onClick?: () => void;
  active?: boolean;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      aria-pressed={active || undefined}
      onMouseDown={(e) => e.preventDefault()}
      onClick={onClick}
      className={`flex h-6 min-w-0 flex-1 items-center justify-center self-center rounded-md ${
        active
          ? "bg-selection text-content"
          : "text-content/50 hover:bg-content/5 hover:text-content"
      }`}
    >
      {children}
    </button>
  );
}

function TreeChildren({
  parent,
  depth,
  entries,
  loading,
  error,
}: {
  parent: string;
  depth: number;
  entries: FsEntry[] | null;
  loading: boolean;
  error: string | null;
}) {
  const ctx = useTree();
  const creating = ctx.creating;
  const show = creating?.parent === parent;
  const row =
    show && creating ? (
      <NameRow
        key={creating.id}
        depth={depth}
        isDir={creating.isDir}
        siblings={(entries ?? []).map((entry) => entry.name)}
        onCommit={(raw) => ctx.onCreateCommit(creating.id, raw)}
        onCancel={() => ctx.onCreateCancel(creating.id)}
      />
    ) : null;
  const { folders, files } = visibleChildren(
    entries ?? [],
    ctx.showExcludedFiles,
  );
  const pad = { paddingLeft: 28 + depth * 12 };

  return (
    <>
      {error ? (
        <p className="truncate pr-2 text-[12px] text-content/50" style={pad}>
          {error}
        </p>
      ) : null}
      {show && ctx.creating?.isDir ? row : null}
      {loading && !error ? (
        <p className="pr-2 text-[12px] text-content/50" style={pad}>
          …
        </p>
      ) : null}
      {folders.map((child) => (
        <TreeNode key={child.path} entry={child} depth={depth} />
      ))}
      {show && ctx.creating && !ctx.creating.isDir ? row : null}
      {files.map((child) => (
        <TreeNode key={child.path} entry={child} depth={depth} />
      ))}
    </>
  );
}

function TreeNode({ entry, depth }: { entry: FsEntry; depth: number }) {
  const {
    expanded,
    selectedPath,
    renaming,
    cutPaths,
    hasSelection,
    isSelected,
    onRowClick,
    dragOverPath,
    epoch,
    gitStatuses,
    onToggle,
    onSelect,
    onFilePointerDown,
    consumeFileClick,
    onOpenFile,
    onRenameCommit,
    onRenameCancel,
    onItemContextMenu,
  } = useTree();
  const open = expanded.has(entry.path);
  const [children, setChildren] = useState<FsEntry[] | null>(() =>
    entry.isDir ? peekDir(entry.path) : null,
  );
  const [error, setError] = useState<string | null>(null);
  const selected =
    isSelected(entry.path) || (!hasSelection && selectedPath === entry.path);
  const editing = renaming === entry.path;
  const gitStatus = entry.isDir
    ? gitStatuses?.dirs.get(entry.path)
    : gitStatuses?.files.get(entry.path);
  const gitColor = gitStatus ? GIT_STATUS_COLOR[gitStatus] : undefined;

  useEffect(() => {
    if (!entry.isDir || !open) return;
    const hit = peekDir(entry.path);
    if (hit) {
      setChildren(hit);
      setError(null);
      return;
    }
    let cancelled = false;
    void listCachedDir(entry.path)
      .then((entries) => {
        if (!cancelled) {
          setChildren(entries);
          setError(null);
        }
      })
      .catch((err: unknown) => {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : String(err));
          setChildren([]);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [entry.isDir, entry.path, open, epoch]);

  const onClick = (event: ReactMouseEvent) => {
    if (consumeFileClick()) return;
    const mode = onRowClick(event, entry.path);
    onSelect(entry.path);
    // Cmd/Ctrl/Shift-click only selects.
    if (mode !== "single") return;
    if (entry.isDir) onToggle(entry.path);
    else onOpenFile(entry.path, undefined, { exact: true });
  };

  const siblings = (peekDir(parentPath(entry.path)) ?? [])
    .map((child) => child.name)
    .filter((name) => name !== entry.name);

  return (
    <div>
      {editing ? (
        <NameRow
          depth={depth}
          isDir={entry.isDir}
          initial={entry.name}
          selectStem={!entry.isDir}
          siblings={siblings}
          onCommit={(raw) => onRenameCommit(entry.path, raw)}
          onCancel={onRenameCancel}
        />
      ) : (
        <button
          type="button"
          role="treeitem"
          title={entry.path}
          aria-expanded={entry.isDir ? open : undefined}
          aria-selected={selected}
          onClick={onClick}
          onMouseDown={(event) => {
            // Shift-click extends the selection, not the page's text selection.
            if (!event.shiftKey) return;
            event.preventDefault();
            event.currentTarget.focus();
          }}
          onDoubleClick={(event) => {
            if (!entry.isDir && selectionMode(event) === "single") {
              onOpenFile(entry.path, undefined, { exact: true, pin: true });
            }
          }}
          onPointerDown={(event) => {
            if (!entry.isDir) onFilePointerDown(entry.path, event);
          }}
          onContextMenu={(e) => onItemContextMenu(entry, e)}
          style={{ paddingLeft: 8 + depth * 12 }}
          className={`flex h-7.5 w-full cursor-default items-center gap-1 pr-2 text-left text-[14px] leading-none data-[explorer-dragging]:opacity-50 ${
            selected
              ? "bg-selection text-content"
              : "text-content hover:bg-content/5"
          } ${cutPaths.includes(entry.path) ? "opacity-50" : ""} ${
            dragOverPath === entry.path ? "bg-selection" : ""
          }`}
        >
          <span className="grid size-4 shrink-0 place-items-center text-content/50">
            {entry.isDir ? (
              open ? (
                <ChevronDown className="size-3.5" strokeWidth={1.75} />
              ) : (
                <ChevronRight className="size-3.5" strokeWidth={1.75} />
              )
            ) : null}
          </span>
          <span className="shrink-0">
            <FileTypeIcon name={entry.name} isDir={entry.isDir} isOpen={open} />
          </span>
          <span
            className={`min-w-0 truncate ${
              entry.ignored ? "italic text-content/50" : (gitColor ?? "")
            }`}
          >
            {entry.name}
          </span>
        </button>
      )}
      {entry.isDir && open ? (
        <TreeChildren
          parent={entry.path}
          depth={depth + 1}
          entries={children}
          loading={children === null && !error}
          error={error}
        />
      ) : null}
    </div>
  );
}

export function NameRow({
  depth,
  isDir,
  initial = "",
  selectStem = false,
  siblings,
  onCommit,
  onCancel,
}: {
  depth: number;
  isDir: boolean;
  initial?: string;
  selectStem?: boolean;
  siblings: string[];
  onCommit: (raw: string) => Promise<void>;
  onCancel: () => void;
}) {
  const inputRef = useRef<HTMLInputElement>(null);
  const finished = useRef(false);
  const [value, setValue] = useState(initial);
  const [attempted, setAttempted] = useState(false);
  const [busy, setBusy] = useState(false);
  const [submitError, setSubmitError] = useState<string | null>(null);
  const issue = validateFileName(value, siblings);
  const leaf = leafName(value);

  useEffect(() => {
    const input = inputRef.current;
    if (!input) return;
    input.focus();
    input.scrollIntoView({ block: "nearest" });
    if (!selectStem) return;
    const dot = initial.lastIndexOf(".");
    if (dot > 0) input.setSelectionRange(0, dot);
    else input.select();
  }, [initial, selectStem]);

  const finish = (success: boolean) => {
    if (finished.current) return;
    if (success) {
      const current = validateFileName(value, siblings);
      if (current && current.severity === "error") {
        setAttempted(true);
        return;
      }
      finished.current = true;
      setBusy(true);
      setSubmitError(null);
      void onCommit(value).catch((err: unknown) => {
        finished.current = false;
        setBusy(false);
        setSubmitError(err instanceof Error ? err.message : String(err));
      });
      return;
    }
    finished.current = true;
    onCancel();
  };

  const showIssue =
    submitError ||
    (issue &&
      (issue.severity === "warning" ||
        (issue.kind !== "empty" && value.length > 0) ||
        (issue.kind === "empty" && attempted)));

  return (
    <div>
      <div
        style={{ paddingLeft: 8 + depth * 12 }}
        className="flex h-7.5 w-full items-center gap-1 bg-content/10 pr-2"
      >
        <span className="grid size-4 shrink-0 place-items-center text-content/50">
          {isDir ? (
            <ChevronRight className="size-3.5" strokeWidth={1.75} />
          ) : null}
        </span>
        <span className="shrink-0">
          <FileTypeIcon name={leaf} isDir={isDir} />
        </span>
        <input
          ref={inputRef}
          value={value}
          disabled={busy}
          spellCheck={false}
          autoComplete="off"
          autoCorrect="off"
          autoCapitalize="off"
          aria-label="Type file name. Press Enter to confirm or Escape to cancel."
          onChange={(e) => {
            setValue(e.target.value);
            setSubmitError(null);
          }}
          onKeyDown={(e) => {
            if (e.nativeEvent.isComposing) return;
            if (e.key === "Enter") {
              e.preventDefault();
              e.stopPropagation();
              finish(true);
            } else if (e.key === "Escape") {
              e.preventDefault();
              e.stopPropagation();
              finish(false);
            }
          }}
          onBlur={() => finish(issue === null || issue.severity !== "error")}
          className="h-5 min-w-0 flex-1 rounded-sm bg-content/10 px-1 text-[14px] leading-none text-content outline-none ring-1 ring-accent"
        />
      </div>
      {showIssue ? (
        <NameIssueView depth={depth} issue={issue} fallback={submitError} />
      ) : null}
    </div>
  );
}

function NameIssueView({
  depth,
  issue,
  fallback,
}: {
  depth: number;
  issue: NameIssue | null;
  fallback: string | null;
}) {
  let body: ReactNode = null;
  if (fallback) {
    body = fallback;
  } else if (issue) {
    switch (issue.kind) {
      case "empty":
        body = "A file or folder name must be provided.";
        break;
      case "slash":
        body = "A file or folder name cannot start with a slash.";
        break;
      case "exists":
        body = (
          <>
            A file or folder <span className="font-semibold">{issue.name}</span>{" "}
            already exists at this location. Please choose a different name.
          </>
        );
        break;
      case "invalid":
        body = (
          <>
            The name <span className="font-semibold">{issue.name}</span> is not
            valid as a file or folder name. Please choose a different name.
          </>
        );
        break;
      case "whitespace":
        body =
          "Leading or trailing whitespace detected in file or folder name.";
        break;
    }
  }
  if (!body) return null;
  const error = Boolean(fallback) || !issue || issue.severity === "error";
  return (
    <p
      className={`pr-2 pb-1 text-[12px] leading-4 ${
        error ? "text-red-400" : "text-amber-400"
      }`}
      style={{ paddingLeft: 28 + depth * 12 }}
    >
      {body}
    </p>
  );
}
