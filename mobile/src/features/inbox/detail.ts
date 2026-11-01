import { useCallback, useEffect, useState, useSyncExternalStore } from "react";

import { useInbox } from "./InboxProvider";
import type {
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
};

type Cached = Pick<DetailState, "detail" | "diff" | "checks">;

/** What each sheet read last, so one opened again shows at once while it refreshes. */
const cache = new Map<string, Cached>();

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

  // The list's copy follows marks made anywhere; the read one is the fallback
  // for an item opened from a push, before any list has it.
  const listed = useSyncExternalStore(store.subscribe, () => store.find(key));
  return {
    ...state,
    item: listed ?? state.detail?.item,
    error: state.detail || listed ? null : error,
    loading,
    reload: useCallback(() => {
      setLoading(true);
      setTick((value) => value + 1);
    }, []),
  };
}
