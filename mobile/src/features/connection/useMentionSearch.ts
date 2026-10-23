import { useCallback, useEffect, useMemo, useSyncExternalStore } from "react";
import { scorePath } from "@/features/files/fuzzy";
import { useConnection } from "./ConnectionProvider";

export type MentionFile = {
  path: string;
  name: string;
  isDir: boolean;
  positions: number[];
  label: string;
  /** Older desktops cannot supply an alias for spaced paths. Send it literally. */
  literalPath?: boolean;
};
type RawFile = {
  path?: string;
  name: string;
  isDir?: boolean;
  positions?: number[];
  label?: string;
  ignored?: boolean;
};
type Answer = { results: MentionFile[]; at: number; error?: string };
type Store = {
  version: number;
  listeners: Set<() => void>;
  root?: Answer;
  rootPending?: Promise<void>;
  queries: Map<string, Answer>;
  pending: Set<string>;
};
const stores = new WeakMap<object, Map<string, Store>>();
const EMPTY: MentionFile[] = [];
const TTL = 30_000;
const DEBOUNCE = 60;
function storeFor(command: object, scope: string): Store {
  let scopes = stores.get(command);
  if (!scopes) stores.set(command, (scopes = new Map()));
  let store = scopes.get(scope);
  if (!store) {
    store = {
      version: 0,
      listeners: new Set(),
      queries: new Map(),
      pending: new Set(),
    };
    scopes.set(scope, store);
    // Retain a small warm cache when moving between chats and projects.
    for (const [key, value] of scopes) {
      if (scopes.size <= 12) break;
      if (!value.listeners.size && key !== scope) scopes.delete(key);
    }
  }
  return store;
}
function notify(store: Store) {
  store.version += 1;
  for (const listener of store.listeners) listener();
}
function normalize(file: RawFile, root = false): MentionFile | null {
  if (file.name === ".git" || file.ignored) return null;
  const path = root ? file.name : file.path;
  if (
    !path ||
    /[\p{Cc}\p{Cf}]/u.test(path) ||
    path.startsWith("/") ||
    path.split("/").some((part) => !part || part === "." || part === "..")
  )
    return null;
  const literalPath = !file.label && /[\s@\\]/.test(path);
  return {
    path,
    name: file.name,
    isDir: !!file.isDir,
    positions: file.positions ?? [],
    // Relative paths are already understood by the desktop's send flow.
    label:
      file.label ??
      (literalPath ? path.split("/").map(encodeURIComponent).join("/") : path),
    ...(literalPath ? { literalPath: true } : {}),
  };
}
function withDirectories(files: MentionFile[]) {
  const paths = new Map(files.map((file) => [file.path, file]));
  for (const file of files) {
    const parts = file.path.split("/");
    for (let i = 1; i < parts.length; i++) {
      const path = parts.slice(0, i).join("/");
      if (!paths.has(path))
        paths.set(path, normalize({ path, name: parts[i - 1], isDir: true })!);
    }
  }
  return [...paths.values()];
}
function rank(files: MentionFile[], query: string): MentionFile[] {
  const needle = query.replace(/\/+$/, "").trim();
  const unique = new Map(files.map((file) => [file.path, file]));
  return [...unique.values()]
    .flatMap((file) => {
      const hit = scorePath(needle, file.path, file.name);
      return hit
        ? [{ ...file, positions: hit.positions, score: hit.score }]
        : [];
    })
    .sort(
      (a, b) =>
        b.score - a.score ||
        a.path.length - b.path.length ||
        a.path.localeCompare(b.path),
    )
    .slice(0, 30);
}
const errorText = (reason: unknown) =>
  reason instanceof Error ? reason.message : String(reason);

type Command = ReturnType<typeof useConnection>["command"];
function readRoot(
  command: Command,
  store: Store,
  projectId: string,
  worktreePath?: string,
) {
  if (
    store.rootPending ||
    (store.root && !store.root.error && Date.now() - store.root.at < TTL)
  )
    return;
  store.rootPending = command<{ entries: RawFile[] }>("project.files", {
    projectId,
    path: "",
    ...(worktreePath ? { worktreePath } : {}),
  })
    .then(({ entries }) => {
      if (!Array.isArray(entries))
        throw new Error("Desktop did not return a folder listing");
      const results = entries.flatMap((file) => {
        const value = normalize(file, true);
        return value ? [value] : [];
      });
      results.sort(
        (a, b) =>
          Number(b.isDir) - Number(a.isDir) || a.name.localeCompare(b.name),
      );
      store.root = { results, at: Date.now() };
    })
    .catch((reason) => {
      store.root = { results: [], at: Date.now(), error: errorText(reason) };
    })
    .finally(() => {
      store.rootPending = undefined;
      notify(store);
    });
}
function readQuery(
  command: Command,
  store: Store,
  projectId: string,
  query: string,
  worktreePath?: string,
) {
  const cached = store.queries.get(query);
  if (
    store.pending.has(query) ||
    (cached && !cached.error && Date.now() - cached.at < TTL)
  )
    return;
  store.pending.add(query);
  void command<{ results: RawFile[] }>("project.search", {
    projectId,
    query,
    mentions: true,
    ...(worktreePath ? { worktreePath } : {}),
  })
    .then(({ results }) => {
      if (!Array.isArray(results))
        throw new Error("Desktop did not return search results");
      const files = results.flatMap((file) => {
        const value = normalize(file);
        return value ? [value] : [];
      });
      store.queries.set(query, {
        results: withDirectories(files),
        at: Date.now(),
      });
    })
    .catch((reason) => {
      store.queries.set(query, {
        results: [],
        at: Date.now(),
        error: errorText(reason),
      });
    })
    .finally(() => {
      store.pending.delete(query);
      while (store.queries.size > 16)
        store.queries.delete(store.queries.keys().next().value!);
      notify(store);
    });
}

/** Warm root browsing; locally filter cached hits while the desktop searches. */
export function useMentionSearch(
  projectId: string | undefined,
  query: string | null,
  worktreePath?: string,
) {
  const { command, pairing, connected } = useConnection();
  const scope = JSON.stringify([
    pairing?.desktop.fingerprint,
    projectId,
    worktreePath,
  ]);
  const store = useMemo(() => storeFor(command, scope), [command, scope]);
  const subscribe = useCallback(
    (listener: () => void) => {
      store.listeners.add(listener);
      return () => {
        store.listeners.delete(listener);
      };
    },
    [store],
  );
  const snapshot = useCallback(() => store.version, [store]);
  const version = useSyncExternalStore(subscribe, snapshot, snapshot);

  // Preload before @ and retry an early connection failure when focus/query
  // activation or reconnection makes the desktop available.
  const active = query !== null;
  useEffect(() => {
    if (projectId && connected !== false)
      readRoot(command, store, projectId, worktreePath);
  }, [command, projectId, worktreePath, store, connected, active]);

  useEffect(() => {
    if (!projectId || !query?.trim() || connected === false) return;
    const cached = store.queries.get(query);
    if (cached && !cached.error && Date.now() - cached.at < TTL) return;
    const timer = setTimeout(
      () => readQuery(command, store, projectId, query, worktreePath),
      DEBOUNCE,
    );
    return () => clearTimeout(timer);
  }, [command, projectId, query, worktreePath, store, connected]);

  const results = useMemo(() => {
    if (!projectId || query === null) return EMPTY;
    if (!query.trim()) return store.root?.results ?? EMPTY;
    const current = store.queries.get(query);
    // Re-filter every candidate for the current query and scope, so previous
    // searches provide instant matches without offering unrelated stale rows.
    const candidates =
      current?.results ??
      [...store.queries.values()].flatMap((answer) => answer.results);
    return rank([...(store.root?.results ?? []), ...candidates], query);
    // The store's monotonically increasing snapshot represents cached changes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [store, version, projectId, query]);
  const current = query?.trim() ? store.queries.get(query) : store.root;
  return {
    results,
    searching: !!projectId && query !== null && !current,
    error: current?.error,
  };
}
