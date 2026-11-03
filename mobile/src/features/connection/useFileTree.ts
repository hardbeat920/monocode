import {
  useCallback,
  useEffect,
  useEffectEvent,
  useMemo,
  useRef,
  useState,
} from "react";
import type { FileEntry } from "@/features/workspace/types";
import { useConnection } from "./ConnectionProvider";

/** A folder's entries once read, or why they couldn't be. */
type Folder = { entries: FileEntry[] } | { error: string };

/** A row of the tree as shown: an entry, its path from the root and its depth. */
export type TreeRow = {
  path: string;
  entry: FileEntry;
  depth: number;
  expanded: boolean;
  /** For a folder, whether its entries have been read, so it can open at once. */
  loaded: boolean;
  /** Asked to open before its entries were read; it opens once they are. */
  loading: boolean;
  error?: string;
};

/** Folders read ahead at once, under the desktop's limit on requests in flight. */
const PREFETCH = 4;

/** Each working copy's tree as last shown, so the Files page comes back as
 *  it was, open folders and all, while it reads them again. */
const lastShown = new Map<string, { folders: Map<string, Folder>; open: Set<string> }>();

/**
 * The project's files as a tree whose folders open in place.
 * Shown folders are read from the desktop a level ahead, ignored
 * ones aside, so most open at once; one asked to open before then opens when
 * its entries arrive. Each is kept, so closing and reopening it is instant; `refresh` reads every open one
 * again, as does coming back to the page. The checkout's, or a worktree's when
 * given its path; switching starts again from the top, closed.
 */
export function useFileTree(
  projectId: string,
  active: boolean,
  worktreePath?: string,
) {
  const { command } = useConnection();
  const scope = `${projectId}\n${worktreePath ?? ""}`;
  const [byPath, setFolders] = useState<Map<string, Folder>>(
    () => lastShown.get(scope)?.folders ?? new Map(),
  );
  const [open, setOpen] = useState<Set<string>>(() => lastShown.get(scope)?.open ?? new Set());
  const [opening, setOpening] = useState<Set<string>>(new Set());
  const [refreshing, setRefreshing] = useState(false);
  // Requests in flight, by scope and path, so a folder isn't asked for twice
  // and opening one already being read waits for that read.
  const pending = useRef(new Map<string, Promise<void>>());
  const latestScope = useRef(scope);
  useEffect(() => {
    latestScope.current = scope;
  }, [scope]);
  // Another project or working copy: its folders aren't these.
  const [shownScope, setShownScope] = useState(scope);
  if (shownScope !== scope) {
    setShownScope(scope);
    setFolders(lastShown.get(scope)?.folders ?? new Map());
    setOpen(lastShown.get(scope)?.open ?? new Set());
    setOpening(new Set());
  }
  useEffect(() => {
    if (shownScope === scope) lastShown.set(scope, { folders: byPath, open });
  }, [scope, shownScope, byPath, open]);

  const load = useCallback(
    (path: string): Promise<void> => {
      const key = `${scope}\n${path}`;
      const inFlight = pending.current.get(key);
      if (inFlight) return inFlight;
      const reading = (async () => {
        let folder: Folder;
        try {
          const value = await command<{ entries: FileEntry[] }>(
            "project.files",
            {
              projectId,
              path,
              ...(worktreePath ? { worktreePath } : {}),
            },
          );
          folder = { entries: value.entries };
        } catch (reason) {
          folder = {
            error: reason instanceof Error ? reason.message : String(reason),
          };
        }
        pending.current.delete(key);
        // Read for a scope since left; its answer doesn't belong here.
        if (latestScope.current !== scope) return;
        setFolders((current) => {
          // A failed refresh keeps what was read before.
          const previous = current.get(path);
          if ("error" in folder && previous && "entries" in previous)
            return current;
          return new Map(current).set(path, folder);
        });
      })();
      pending.current.set(key, reading);
      return reading;
    },
    [command, projectId, worktreePath, scope],
  );

  // The root, and every open folder again, each time the page is shown.
  const refreshAll = useCallback(
    () => Promise.all(["", ...open].map(load)).then(() => undefined),
    [load, open],
  );
  const refreshOnShow = useEffectEvent(() => void refreshAll());
  useEffect(() => {
    if (active) refreshOnShow();
  }, [active, scope]);

  const setIn = (
    set: (update: (current: Set<string>) => Set<string>) => void,
    path: string,
    on: boolean,
  ) =>
    set((current) => {
      const next = new Set(current);
      if (on) next.add(path);
      else next.delete(path);
      return next;
    });

  const toggle = useCallback(
    async (path: string) => {
      if (open.has(path)) return setIn(setOpen, path, false);
      if (byPath.has(path)) return setIn(setOpen, path, true);
      // Not read yet: it waits, marked as opening, and opens once it is.
      if (opening.has(path)) return;
      const asked = scope;
      setIn(setOpening, path, true);
      await load(path);
      if (latestScope.current !== asked) return;
      setIn(setOpening, path, false);
      setIn(setOpen, path, true);
    },
    [open, byPath, opening, load, scope],
  );

  const refresh = useCallback(async () => {
    setRefreshing(true);
    await refreshAll();
    setRefreshing(false);
  }, [refreshAll]);

  // Depth first: each open folder's entries right under it.
  const rows = useMemo(() => {
    const out: TreeRow[] = [];
    const walk = (dir: string, depth: number) => {
      const folder = byPath.get(dir);
      if (!folder || !("entries" in folder)) return;
      for (const entry of folder.entries) {
        const path = dir ? `${dir}/${entry.name}` : entry.name;
        const isOpen = entry.isDir && open.has(path);
        const child = byPath.get(path);
        out.push({
          path,
          entry,
          depth,
          expanded: isOpen,
          loaded: !!child,
          loading: opening.has(path),
          ...(isOpen && child && "error" in child
            ? { error: child.error }
            : {}),
        });
        if (isOpen) walk(path, depth + 1);
      }
    };
    walk("", 0);
    return out;
  }, [byPath, open, opening]);

  // A level ahead: the shown folders not read yet, a few at a time. Each read
  // that lands changes the rows, which reads the next ones.
  useEffect(() => {
    if (!active) return;
    const inFlight = [...pending.current.keys()].filter((key) =>
      key.startsWith(`${scope}\n`),
    ).length;
    rows
      .filter(
        (row) =>
          row.entry.isDir &&
          !row.entry.ignored &&
          !row.loaded &&
          !pending.current.has(`${scope}\n${row.path}`),
      )
      .slice(0, Math.max(0, PREFETCH - inFlight))
      .forEach((row) => void load(row.path));
  }, [active, rows, scope, load]);

  const root = byPath.get("");
  return {
    rows,
    /** Whether the root has been read, successfully or not. */
    ready: !!root,
    error: root && "error" in root ? root.error : null,
    toggle,
    refresh,
    refreshing,
  };
}
