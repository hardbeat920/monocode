import { deepEqual } from "@/features/connection/deepEqual";

import {
  applyMark,
  facetParams,
  feedKey,
  hasFacets,
  marksToRestore,
  parseCachedPage,
  type InboxFacets,
  type Mark,
} from "./model";
import {
  PAGE_SIZE,
  type InboxAction,
  type InboxActionResult,
  type InboxClient,
  type InboxItem,
  type InboxList,
  type InboxMarkAction,
  type InboxProviderState,
  type InboxScope,
  type InboxSource,
} from "./protocol";

/** What a list asks the desktop for. */
export type FeedQuery = {
  scope: InboxScope;
  projectId?: string;
  source?: InboxSource;
  q?: string;
  facets?: InboxFacets;
};

/** A filtered or searched list is passing, not worth one of the cached places. */
function kept(query: FeedQuery) {
  return !query.q && !hasFacets(query.facets);
}

/** One list as the screens see it: items with pending marks laid over them. */
export type Feed = {
  key: string;
  items: InboxItem[];
  /** Something has been read, from the desktop or the phone's cache. */
  loaded: boolean;
  loadingMore: boolean;
  /** What shows came from the phone's cache and the desktop hasn't answered yet. */
  fromCache: boolean;
  /** The last read failed, or the desktop isn't linked. */
  offline: boolean;
  error: string | null;
  nextCursor?: string;
  /** The desktop calls its snapshot stale. */
  stale: boolean;
  fetchedAt: number | null;
  providers: InboxProviderState[];
  counts: InboxList["counts"];
  options?: InboxList["options"];
};

type Raw = Omit<Feed, "items" | "key"> & { items: InboxItem[] };

/** Where the last page is kept; the app's key-value store in the app. */
export type InboxStorage = {
  get: (key: string) => string | null;
  set: (key: string, value: string) => void;
};

export type InboxStoreOptions = {
  client: InboxClient;
  storage: InboxStorage;
  /** Namespaces the cache by desktop. */
  namespace: string;
  /** As often as the workspace catalog: every 5 seconds while a list is in view. */
  pollMs?: number;
  /** False while the app is in the background: nothing is polled then. */
  isActive?: () => boolean;
};

export const POLL_MS = 5000;
/**
 * How often with a desktop that says when to read. Only for what its rev can't
 * see, as a snooze running out.
 */
export const FOLLOWED_POLL_MS = 5 * 60_000;
/** The lists kept on the phone per desktop; the least recently read go. */
const CACHE_LIMIT = 8;
/** Pages re-read in one refresh to cover what was scrolled in; the rest is dropped. */
const MAX_REREAD_PAGES = 5;

const EMPTY_COUNTS = { unread: 0, needsMe: 0, done: 0 };

/** What a mark did, and the way back. */
export type MarkResult = {
  /** Puts the item back as it was; the desktop is told too. */
  undo: () => Promise<void>;
};

/**
 * The Inbox's state on the phone: lists by query, kept in step with the
 * desktop by polling while one is watched, cached locally for the next
 * launch, with marks shown at once and rolled back if the desktop refuses.
 * Plain TypeScript with an injected client and storage, so it runs under test
 * and against the fixture client alike; the provider hands it to React.
 */
export function createInboxStore(options: InboxStoreOptions) {
  const { client, storage, namespace } = options;
  const pollMs = options.pollMs ?? POLL_MS;
  const isActive = options.isActive ?? (() => true);

  const raws = new Map<string, Raw>();
  const queries = new Map<string, FeedQuery>();
  const views = new Map<string, Feed>();
  /** Items as a pending mark leaves them. */
  let overrides: Record<string, InboxItem> = {};
  /** Items the phone has learned from a mark's reply, for a sheet opened from a push. */
  const known = new Map<string, InboxItem>();
  const listeners = new Set<() => void>();
  const watching = new Map<string, { count: number; timer?: ReturnType<typeof setTimeout> }>();
  const inflight = new Map<string, Promise<void>>();
  /** Bumped when a refresh replaces a list, so a page asked for before it is dropped. */
  const generations = new Map<string, number>();
  let linked = true;
  let disposed = false;
  /** The desktop's Inbox rev, once it publishes one. */
  let followed: string | null = null;

  const cacheKey = (key: string) => `monocode.inbox.${namespace}.${key}`;
  const indexKey = `monocode.inbox.${namespace}.index`;

  function emit() {
    for (const listener of listeners) listener();
  }

  function raw(query: FeedQuery): Raw {
    const key = feedKey(query);
    queries.set(key, query);
    let value = raws.get(key);
    if (!value) {
      const cached = kept(query) ? parseCachedPage(storage.get(cacheKey(key))) : null;
      value = {
        items: cached?.items ?? [],
        loaded: !!cached,
        loadingMore: false,
        fromCache: !!cached,
        offline: false,
        error: null,
        nextCursor: cached?.nextCursor,
        stale: false,
        fetchedAt: cached?.fetchedAt ?? null,
        providers: cached?.providers ?? [],
        counts: cached?.counts ?? EMPTY_COUNTS,
        options: cached?.options,
      };
      raws.set(key, value);
    }
    return value;
  }

  /** Rebuilds the view of `key` if what it shows changed. */
  function view(key: string): Feed {
    const value = raws.get(key)!;
    const previous = views.get(key);
    const items = value.items.map((item) => overrides[item.key] ?? item);
    if (
      previous &&
      previous.items.length === items.length &&
      previous.items.every((item, index) => item === items[index]) &&
      sameMeta(previous, value)
    ) {
      return previous;
    }
    const next: Feed = { ...value, key, items };
    views.set(key, next);
    return next;
  }

  function update(key: string, change: Partial<Raw>) {
    const value = raws.get(key);
    if (!value) return;
    raws.set(key, { ...value, ...change });
    view(key);
    emit();
  }

  function remember(key: string, page: Raw) {
    storage.set(
      cacheKey(key),
      JSON.stringify({
        items: page.items,
        nextCursor: page.nextCursor,
        fetchedAt: page.fetchedAt,
        providers: page.providers,
        counts: page.counts,
        options: page.options,
      }),
    );
    let index: string[] = [];
    try {
      index = JSON.parse(storage.get(indexKey) ?? "[]");
    } catch {
      index = [];
    }
    index = [key, ...index.filter((entry) => entry !== key)];
    for (const old of index.splice(CACHE_LIMIT)) storage.set(cacheKey(old), "");
    storage.set(indexKey, JSON.stringify(index));
  }

  function params(query: FeedQuery, cursor?: string, refresh?: boolean) {
    return {
      ...(refresh ? { refresh: true } : {}),
      scope: query.scope,
      ...(query.projectId ? { projectId: query.projectId } : {}),
      ...(query.source ? { source: query.source } : {}),
      ...(query.q ? { q: query.q } : {}),
      ...facetParams(query.facets, Date.now()),
      ...(cursor ? { cursor } : {}),
      limit: PAGE_SIZE,
    };
  }

  async function read(query: FeedQuery, force = false): Promise<void> {
    const key = feedKey(query);
    const before = raw(query).items.length;
    if (!linked) {
      update(key, { offline: true });
      return;
    }
    try {
      const page = await client.list(params(query, undefined, force));
      if (disposed) return;
      // Pages scrolled in are read again, not kept: an item the desktop has
      // since finished or dropped must leave, and one that changed must change.
      let items = page.items;
      let cursor = page.nextCursor;
      for (let pages = 1; cursor && items.length < before && pages < MAX_REREAD_PAGES; pages++) {
        const more = await client.list(params(query, cursor));
        if (disposed) return;
        const have = new Set(items.map((item) => item.key));
        items = [...items, ...more.items.filter((item) => !have.has(item.key))];
        cursor = more.nextCursor;
      }
      const previous = raws.get(key)!;
      const next: Raw = {
        ...previous,
        items: sameItems(previous.items, items),
        loaded: true,
        fromCache: false,
        offline: false,
        error: null,
        nextCursor: cursor,
        stale: page.stale,
        fetchedAt: page.fetchedAt,
        providers: same(previous.providers, page.providers),
        counts: same(previous.counts, page.counts),
        options: same(previous.options, page.options),
      };
      const shown = views.get(key);
      raws.set(key, next);
      generations.set(key, (generations.get(key) ?? 0) + 1);
      for (const item of next.items) known.set(item.key, item);
      // Most reads find what was there: the screens and the disk hear of none of those.
      if (view(key) === shown) return;
      if (kept(query)) remember(key, { ...next, items: page.items, nextCursor: page.nextCursor });
      emit();
    } catch (reason) {
      if (disposed) return;
      update(key, {
        offline: true,
        error: reason instanceof Error ? reason.message : String(reason),
      });
    }
  }

  /** Reads a list now. `force` makes the desktop fetch from its providers first. */
  function refresh(query: FeedQuery, options: { force?: boolean } = {}): Promise<void> {
    const key = feedKey(query);
    const running = inflight.get(key);
    // A pull to refresh isn't answered by a poll already on its way: it runs after it.
    if (running && !options.force) return running;
    const run: Promise<void> = (running ?? Promise.resolve())
      .then(() => read(query, options.force))
      .finally(() => {
        if (inflight.get(key) === run) inflight.delete(key);
      });
    inflight.set(key, run);
    return run;
  }

  async function loadMore(query: FeedQuery): Promise<void> {
    const key = feedKey(query);
    const value = raw(query);
    if (!value.nextCursor || value.loadingMore || !linked) return;
    const generation = generations.get(key) ?? 0;
    update(key, { loadingMore: true });
    try {
      const page = await client.list(params(query, value.nextCursor));
      if (disposed) return;
      if ((generations.get(key) ?? 0) !== generation) {
        // A refresh replaced the list while this page was on its way.
        update(key, { loadingMore: false });
        return;
      }
      const latest = raws.get(key)!;
      const have = new Set(latest.items.map((item) => item.key));
      update(key, {
        items: [...latest.items, ...page.items.filter((item) => !have.has(item.key))],
        nextCursor: page.nextCursor,
        loadingMore: false,
      });
    } catch {
      update(key, { loadingMore: false });
    }
  }

  function schedule(key: string) {
    const entry = watching.get(key);
    if (!entry || disposed) return;
    clearTimeout(entry.timer);
    entry.timer = setTimeout(async () => {
      const query = queries.get(key);
      if (query && isActive()) await refresh(query);
      schedule(key);
    }, followed ? FOLLOWED_POLL_MS : pollMs);
  }

  /**
   * The rev the desktop publishes with its catalog: lists in view are read
   * when it moves, in place of polling. A desktop without one is polled.
   */
  function follow(rev: string | null | undefined) {
    const next = rev ?? null;
    if (next === followed) return;
    followed = next;
    for (const key of watching.keys()) {
      schedule(key);
      const query = queries.get(key);
      if (next && query && isActive()) void refresh(query);
    }
  }

  /** Reads a list now and keeps it fresh until the returned function is called. */
  function watch(query: FeedQuery): () => void {
    const key = feedKey(query);
    raw(query);
    const entry = watching.get(key) ?? { count: 0 };
    entry.count++;
    watching.set(key, entry);
    if (entry.count === 1) {
      void refresh(query);
      schedule(key);
    }
    let released = false;
    return () => {
      if (released) return;
      released = true;
      const current = watching.get(key);
      if (!current) return;
      current.count--;
      if (current.count <= 0) {
        clearTimeout(current.timer);
        watching.delete(key);
      }
    };
  }

  // -- Marks ----------------------------------------------------------------

  function find(key: string): InboxItem | undefined {
    if (overrides[key]) return overrides[key];
    for (const value of raws.values()) {
      const item = value.items.find((entry) => entry.key === key);
      if (item) return item;
    }
    return known.get(key);
  }

  function showOptimistic(item: InboxItem) {
    overrides = { ...overrides, [item.key]: item };
    for (const key of raws.keys()) view(key);
    emit();
  }

  function settle(item: InboxItem | null, key: string) {
    const { [key]: _, ...rest } = overrides;
    overrides = rest;
    if (item) {
      known.set(key, item);
      for (const [feed, value] of raws) {
        if (value.items.some((entry) => entry.key === key)) {
          raws.set(feed, {
            ...value,
            items: value.items.map((entry) => (entry.key === key ? item : entry)),
          });
        }
      }
    }
    for (const feed of raws.keys()) view(feed);
    emit();
  }

  /** Sends marks in turn; the last reply is the item as the desktop has it. */
  async function send(key: string, marks: Mark[]): Promise<InboxItem | null> {
    let latest: InboxItem | null = null;
    for (const mark of marks) latest = await client.mark(key, mark.action, mark.until);
    return latest;
  }

  async function mark(
    key: string,
    action: InboxMarkAction,
    until?: number,
  ): Promise<MarkResult> {
    const before = find(key);
    if (!before) throw new Error("That item is not in the inbox");
    const after = applyMark(before, action, until);
    showOptimistic(after);
    try {
      settle(await send(key, [{ action, until }]), key);
    } catch (reason) {
      // The desktop refused, or can't be reached: the item goes back.
      settle(before, key);
      throw reason;
    }
    return {
      undo: async () => {
        const current = find(key) ?? after;
        const marks = marksToRestore(current, before);
        showOptimistic(before);
        try {
          settle(await send(key, marks), key);
        } catch (reason) {
          settle(current, key);
          throw reason;
        }
      },
    };
  }

  /** Runs at the provider, so nothing shows until the desktop says it happened. */
  async function act(key: string, action: InboxAction): Promise<InboxActionResult> {
    const result = await client.act(key, action);
    settle(result.item, key);
    return result;
  }

  /** Marks read what the list holds; a refused request changes nothing here. */
  async function readAll(query: FeedQuery): Promise<void> {
    await client.readAll(params(query));
    const key = feedKey(query);
    const value = raws.get(key);
    if (value) {
      update(key, {
        items: value.items.map((item) => ({ ...item, unread: false })),
        counts: { ...value.counts, unread: 0 },
      });
    }
    await refresh(query);
  }

  return {
    follow,
    getFeed(query: FeedQuery): Feed {
      raw(query);
      return view(feedKey(query));
    },
    subscribe(listener: () => void) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    watch,
    refresh,
    loadMore,
    mark,
    act,
    readAll,
    find,
    /** Whether the desktop is linked; polls stop reading while it isn't. */
    setLinked(next: boolean) {
      if (linked === next) return;
      linked = next;
      for (const [key, value] of raws) {
        if (!next && !value.offline) update(key, { offline: true });
      }
      if (next) for (const query of queries.values()) if (watching.has(feedKey(query))) void refresh(query);
    },
    /** Reads every watched list now, as when the app comes back to the front. */
    wake() {
      for (const key of watching.keys()) {
        const query = queries.get(key);
        if (query) void refresh(query);
      }
    },
    dispose() {
      disposed = true;
      for (const entry of watching.values()) clearTimeout(entry.timer);
      watching.clear();
      listeners.clear();
    },
  };
}

export type InboxStore = ReturnType<typeof createInboxStore>;

/** `next`, or `previous` when they say the same, so nothing downstream sees a change. */
function same<T>(previous: T, next: T): T {
  return deepEqual(previous, next) ? previous : next;
}

/**
 * The items of a read, each still the object it was unless it changed. Every
 * reply arrives as new objects, and a row is only spared a redraw by being
 * handed the same one.
 */
function sameItems(previous: InboxItem[], next: InboxItem[]) {
  const known = new Map(previous.map((item) => [item.key, item]));
  return next.map((item) => {
    const before = known.get(item.key);
    return before && deepEqual(before, item) ? before : item;
  });
}

function sameMeta(feed: Feed, value: Raw) {
  return (
    feed.loaded === value.loaded &&
    feed.loadingMore === value.loadingMore &&
    feed.fromCache === value.fromCache &&
    feed.offline === value.offline &&
    feed.error === value.error &&
    feed.nextCursor === value.nextCursor &&
    feed.stale === value.stale &&
    feed.fetchedAt === value.fetchedAt &&
    feed.providers === value.providers &&
    feed.counts === value.counts &&
    feed.options === value.options
  );
}
