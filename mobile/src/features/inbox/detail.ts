import { useCallback, useEffect, useState, useSyncExternalStore } from "react";

import { useInbox } from "./InboxProvider";
import type {
  InboxActionResult,
  InboxCheck,
  InboxDetail,
  InboxDiff,
  InboxItem,
} from "./protocol";

/** Checks are GitHub pull requests' only. */
export function hasChecks(item: Pick<InboxItem, "source" | "kind">) {
  return item.source === "github" && item.kind === "pr";
}

/** Only pull requests have code. */
export function hasCode(item: Pick<InboxItem, "kind">) {
  return item.kind === "pr";
}

export type DetailState = {
  /** The item as the list has it, or as the sheet read it. */
  item: InboxItem | undefined;
  detail: InboxDetail | null;
  diff: InboxDiff | null;
  checks: InboxCheck[] | null;
  /** The item itself couldn't be read and isn't in the list either. */
  error: string | null;
  /** Something is still being read. */
  loading: boolean;
  reload: () => void;
  /** Shows what an action's reply says at once, then reads the item again. */
  adopt: (result: InboxActionResult) => void;
};

type Cached = Pick<DetailState, "detail" | "diff" | "checks">;

/** What each sheet read last, so one opened again shows at once while it refreshes. */
const cache = new Map<string, Cached>();

/** The open sheets, by item, for a change made on another screen. */
const open = new Map<string, Set<() => void>>();

/** Reads an item's open sheets again; a posted comment shows this way. */
export function reloadInboxDetail(key: string) {
  for (const reload of open.get(key) ?? []) reload();
}

/**
 * An item's sheet: its body and thread, then (for pull requests) its diff and
 * (for GitHub's) its checks. Read together; a part that fails leaves the
 * others. The last read shows first.
 */
export function useInboxDetail(key: string): DetailState {
  const { client, store } = useInbox();
  const [state, setState] = useState<Cached>(() => cache.get(key) ?? { detail: null, diff: null, checks: null });
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [tick, setTick] = useState(0);

  useEffect(() => {
    let cancelled = false;
    const keep = (part: Partial<Cached>) => {
      const next = { ...(cache.get(key) ?? { detail: null, diff: null, checks: null }), ...part };
      cache.set(key, next);
      if (!cancelled) setState(next);
    };
    void (async () => {
      try {
        const detail = await client.item(key);
        if (cancelled) return;
        setError(null);
        keep({ detail });
        const more: Promise<void>[] = [];
        if (hasCode(detail.item))
          more.push(client.diff(key).then((diff) => keep({ diff }), () => {}));
        if (hasChecks(detail.item))
          more.push(client.checks(key).then((checks) => keep({ checks }), () => {}));
        await Promise.all(more);
      } catch (reason) {
        if (!cancelled) setError(reason instanceof Error ? reason.message : String(reason));
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [client, key, tick]);

  const reload = useCallback(() => {
    setLoading(true);
    setTick((value) => value + 1);
  }, []);

  useEffect(() => {
    const sheets = open.get(key) ?? new Set();
    open.set(key, sheets.add(reload));
    return () => {
      sheets.delete(reload);
      if (sheets.size === 0) open.delete(key);
    };
  }, [key, reload]);

  const adopt = useCallback(
    ({ item, actions }: InboxActionResult) => {
      const current = cache.get(key);
      if (current?.detail) {
        const next = { ...current, detail: { ...current.detail, item, actions } };
        cache.set(key, next);
        setState(next);
      }
      reload();
    },
    [key, reload],
  );

  // The list's copy follows marks made anywhere; the read one is the fallback
  // for an item opened from a push, before any list has it.
  const listed = useSyncExternalStore(store.subscribe, () => store.find(key));
  return {
    ...state,
    item: listed ?? state.detail?.item,
    error: state.detail || listed ? null : error,
    loading,
    reload,
    adopt,
  };
}
