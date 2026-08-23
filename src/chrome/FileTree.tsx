import {
  ChevronDown,
  ChevronRight,
  FilePlus,
  FolderPlus,
  FoldVertical,
  Search,
} from "lucide-react";
import {
  createContext,
  useContext,
  useEffect,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type MouseEvent as ReactMouseEvent,
  type ReactNode,
} from "react";
import {
  leafName,
  validateFileName,
  wellFormedFileName,
  type NameIssue,
} from "../lib/fileName";
import {
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
} from "../lib/fileTree";
import {
  basename,
  copyPath,
  createPath,
  deletePath,
  movePath,
  renamePath,
  revealPath,
  type FsEntry,
} from "../lib/fs";
import { displayPath, parentPath, rebasePath } from "../lib/paths";
import { IS_MAC, MOD } from "../lib/platform";
import type { GitStatusMap } from "../hooks/useGitFileStatuses";
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
  onOpenFile: (path: string) => void;
  onOpenTerminal?: (cwd: string) => void;
  onFileMoved?: (from: string, to: string) => void;
  onFileDeleted?: (path: string) => void;
  onSearch?: () => void;
  headerEnd?: ReactNode;
  gitStatuses?: GitStatusMap;
};

type Creating = { id: number; parent: string; isDir: boolean };
type Clip = { mode: "copy" | "cut"; path: string; isDir: boolean };
type MenuTarget = { path: string; isDir: boolean; isRoot: boolean };
type MenuState = { x: number; y: number; target: MenuTarget };

const REVEAL_LABEL = IS_MAC
  ? "Reveal in Finder"
  : typeof navigator !== "undefined" && /Win/.test(navigator.platform)
    ? "Reveal in File Explorer"
    : "Open Containing Folder";

type TreeCtxValue = {
  expanded: Set<string>;
  selectedPath: string | null;
  creating: Creating | null;
  renaming: string | null;
  cutPath: string | null;
  epoch: number;
  gitStatuses?: GitStatusMap;
  onToggle: (path: string) => void;
  onSelect: (path: string) => void;
  onOpenFile: (path: string) => void;
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

function explorerItems(
  target: MenuTarget,
  clip: Clip | null,
  canOpenTerminal: boolean,
): ExplorerMenuItem[] {
  const pasteParent = target.isDir ? target.path : parentPath(target.path);
  const pasteBlocked =
    !clip ||
    (clip.isDir &&
      (pasteParent === clip.path || pasteParent.startsWith(`${clip.path}/`)));
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
    { kind: "item", id: "copy-path", label: "Copy Path" },
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
      ? [{ kind: "item" as const, id: "open-terminal", label: "Open in Terminal" }]
      : []),
    { kind: "item", id: "reveal", label: REVEAL_LABEL },
  ];
}

export function FileTree({
  cwd,
  onOpenFile,
  onOpenTerminal,
  onFileMoved,
  onFileDeleted,
  onSearch,
  headerEnd,
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
  const [opError, setOpError] = useState<string | null>(null);
  const [epoch, setEpoch] = useState(0);
  const creatingRef = useRef(creating);
  creatingRef.current = creating;
  const rootRef = useRef<HTMLDivElement>(null);
  const name = basename(cwd);
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
      cur && (cur.path === from || cur.path.startsWith(`${from}/`))
        ? { ...cur, path: rebasePath(cur.path, from, to) }
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
    setSelectedPath(created);
    saveSelected(cwd, created);
    if (!asFolder) onOpenFile(created);
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

  const removeEntry = async (path: string) => {
    if (path === cwd) return;
    const isDir = isDirAt(cwd, path);
    const label = basename(path);
    const ok = window.confirm(
      isDir
        ? `Delete folder “${label}” and everything inside it?`
        : `Delete “${label}”?`,
    );
    if (!ok) return;
    await deletePath(path);
    await refreshTouched([parentPath(path)], isDir ? [path] : []);
    setSelectedPath((prev) => {
      if (!prev || prev === path || prev.startsWith(`${path}/`)) {
        const parent = parentPath(path);
        saveSelected(cwd, parent);
        return parent;
      }
      return prev;
    });
    setClip((cur) =>
      cur && (cur.path === path || cur.path.startsWith(`${path}/`))
        ? null
        : cur,
    );
    onFileDeleted?.(path);
  };

  const pasteAt = async (targetPath: string) => {
    if (!clip) return;
    const destParent = createParentOf(cwd, targetPath);
    if (
      clip.isDir &&
      (destParent === clip.path || destParent.startsWith(`${clip.path}/`))
    ) {
      throw new Error("Cannot paste a folder into itself.");
    }
    const from = clip.path;
    const mode = clip.mode;
    const isDir = clip.isDir;
    const created =
      mode === "cut"
        ? await movePath(from, destParent)
        : await copyPath(from, destParent);
    if (mode === "cut") {
      await refreshTouched(
        dirsTouchedByMove(from, created),
        isDir ? [from] : [],
      );
      remapTreePaths(from, created);
      onFileMoved?.(from, created);
      setClip(null);
    } else {
      await refreshTouched([destParent]);
    }
    expandDirs([destParent]);
    setSelectedPath(created);
    saveSelected(cwd, created);
  };

  const duplicateAt = async (path: string) => {
    if (path === cwd) return;
    const destParent = parentPath(path);
    const created = await copyPath(path, destParent);
    await refreshTouched([destParent]);
    setSelectedPath(created);
    saveSelected(cwd, created);
  };

  const run = async (work: () => Promise<void>) => {
    setOpError(null);
    try {
      await work();
    } catch (err: unknown) {
      setOpError(err instanceof Error ? err.message : String(err));
    }
  };

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
        setClip({ mode: "cut", path: target.path, isDir: target.isDir });
        return;
      case "copy":
        if (target.isRoot) return;
        setClip({ mode: "copy", path: target.path, isDir: target.isDir });
        return;
      case "paste":
        await run(() => pasteAt(target.path));
        return;
      case "duplicate":
        await run(() => duplicateAt(target.path));
        return;
      case "copy-path":
        await copyText(target.path);
        return;
      case "copy-relative-path":
        await copyText(displayPath(target.path, cwd));
        return;
      case "rename":
        startRename(target.path);
        return;
      case "delete":
        await run(() => removeEntry(target.path));
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
    openMenu(
      { path: entry.path, isDir: entry.isDir, isRoot: false },
      e.clientX,
      e.clientY,
    );
  };

  const onBackgroundMenu = (e: ReactMouseEvent) => {
    if ((e.target as HTMLElement).closest("input")) return;
    e.preventDefault();
    openMenu({ path: cwd, isDir: true, isRoot: true }, e.clientX, e.clientY);
  };

  const onKeyDown = (e: ReactKeyboardEvent<HTMLDivElement>) => {
    if ((e.target as HTMLElement).closest("input")) return;
    if (
      (e.target as HTMLElement).closest("button") &&
      !(e.target as HTMLElement).closest("[role='treeitem']")
    ) {
      return;
    }
    const path = selectedPath ?? cwd;
    const isRoot = path === cwd;
    const isDir = isDirAt(cwd, path);
    const mod = e.metaKey || e.ctrlKey;
    const key = e.key.toLowerCase();
    if (mod && !e.altKey && !e.shiftKey && key === "c") {
      if (isRoot) return;
      e.preventDefault();
      setClip({ mode: "copy", path, isDir });
      return;
    }
    if (mod && !e.altKey && !e.shiftKey && key === "x") {
      if (isRoot) return;
      e.preventDefault();
      setClip({ mode: "cut", path, isDir });
      return;
    }
    if (mod && !e.altKey && !e.shiftKey && key === "v") {
      e.preventDefault();
      void run(() => pasteAt(path));
      return;
    }
    if (e.key === "F2") {
      e.preventDefault();
      startRename(path);
      return;
    }
    if (e.key === "Delete" || e.key === "Backspace") {
      e.preventDefault();
      void run(() => removeEntry(path));
      return;
    }
    if (e.key === "Escape" && clip?.mode === "cut") {
      e.preventDefault();
      setClip(null);
    }
  };

  useEffect(() => {
    if (!menu) return;
    const onScroll = () => setMenu(null);
    const scrollParent = rootRef.current?.closest(".overflow-y-auto") ?? window;
    scrollParent.addEventListener("scroll", onScroll, true);
    return () => scrollParent.removeEventListener("scroll", onScroll, true);
  }, [menu]);

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
        cutPath: clip?.mode === "cut" ? clip.path : null,
        epoch,
        gitStatuses,
        onToggle: toggle,
        onSelect,
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
        className="min-h-full outline-none"
        onKeyDown={onKeyDown}
        onContextMenu={onBackgroundMenu}
      >
        <div
          className="flex shrink-0 items-center"
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
          {headerEnd}
        </div>
        <div className="flex h-8 items-center sticky top-0 bg-content/5 backdrop-blur-md">
          <button
            type="button"
            aria-expanded={rootOpen}
            title={cwd}
            onClick={() => {
              onSelect(cwd);
              toggle(cwd);
            }}
            onContextMenu={(e) => {
              e.preventDefault();
              e.stopPropagation();
              openMenu(
                { path: cwd, isDir: true, isRoot: true },
                e.clientX,
                e.clientY,
              );
            }}
            className={`flex min-w-0 flex-1 items-center gap-1 h-full pl-2 text-left`}
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
        {opError ? (
          <p className="px-3 py-1 text-[12px] leading-4 text-red-400">
            {opError}
          </p>
        ) : null}
        {rootOpen ? (
          <div role="tree" aria-label={`${name} files`}>
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
}

function HeaderIcon({
  label,
  onClick,
  children,
}: {
  label: string;
  onClick?: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      onMouseDown={(e) => e.preventDefault()}
      onClick={onClick}
      className="flex-1 h-8 place-items-center text-content/50 hover:bg-content/10 hover:text-content flex items-center justify-center"
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
  const folders = entries?.filter((e) => e.isDir) ?? [];
  const files = entries?.filter((e) => !e.isDir) ?? [];
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
    cutPath,
    epoch,
    gitStatuses,
    onToggle,
    onSelect,
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
  const selected = selectedPath === entry.path;
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

  const onClick = () => {
    onSelect(entry.path);
    if (entry.isDir) onToggle(entry.path);
    else onOpenFile(entry.path);
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
          onClick={onClick}
          onContextMenu={(e) => onItemContextMenu(entry, e)}
          style={{ paddingLeft: 8 + depth * 12 }}
          className={`flex h-7.5 w-full cursor-default items-center gap-1 pr-2 text-left text-[14px] leading-none ${
            selected
              ? "bg-content/10 text-content"
              : "text-content hover:bg-content/5"
          } ${cutPath === entry.path ? "opacity-50" : ""}`}
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
              entry.ignored
                ? "italic text-content/50"
                : gitColor ?? ""
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

function NameRow({
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
