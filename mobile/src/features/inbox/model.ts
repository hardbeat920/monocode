import type {
  InboxAttention,
  InboxItem,
  InboxList,
  InboxMarkAction,
  InboxScope,
  InboxSource,
} from "./protocol";

// The Inbox's pure logic: what a scope shows, how rows group, how a mark
// changes an item and is undone, and what the banner above the list says.

const DAY = 24 * 60 * 60 * 1000;

/** The kinds of attention that put an item in "Needs me". */
const NEEDS_ME: ReadonlySet<InboxAttention> = new Set([
  "reviewRequested",
  "mentioned",
  "assigned",
  "ciFailed",
]);

export function needsMe(item: InboxItem) {
  return !item.done && !!item.attention && NEEDS_ME.has(item.attention);
}

/** Snoozed items rest out of sight until their time comes. */
export function isSnoozed(item: InboxItem, now: number) {
  return item.snoozedUntil !== undefined && item.snoozedUntil > now;
}

/** The chips under the switch; each narrows the list on its own. */
export type InboxChip = "unread" | "reviews" | "issues" | "ci";
export const INBOX_CHIPS: { id: InboxChip; label: string }[] = [
  { id: "unread", label: "Unread" },
  { id: "reviews", label: "Reviews" },
  { id: "issues", label: "Issues" },
  { id: "ci", label: "CI failing" },
];

export type InboxFilter = {
  scope: InboxScope;
  /** Only this phone project's items; every project's when unset. */
  projectId?: string;
  chips?: readonly InboxChip[];
  sources?: readonly InboxSource[];
  query?: string;
};

/**
 * Items in `projectId`. An item the desktop couldn't map to a project is
 * left out of a project's list and only shows with all projects.
 */
export function inProject(items: InboxItem[], projectId: string | undefined) {
  return projectId ? items.filter((item) => item.projectId === projectId) : items;
}

function inScope(item: InboxItem, scope: InboxScope, now: number) {
  if (scope === "done") return item.done;
  if (item.done || isSnoozed(item, now)) return false;
  return scope === "needsMe" ? needsMe(item) : true;
}

function matchesChip(item: InboxItem, chip: InboxChip) {
  switch (chip) {
    case "unread":
      return item.unread;
    case "reviews":
      return item.attention === "reviewRequested";
    case "issues":
      return item.kind === "issue";
    case "ci":
      return item.attention === "ciFailed" || item.checks === "failing";
  }
}

/** What the list shows: the scope, project, chips, sources and search applied, newest first. */
export function visibleItems(items: InboxItem[], filter: InboxFilter, now: number) {
  const needle = filter.query?.trim().toLowerCase() ?? "";
  const chips = filter.chips ?? [];
  const sources = filter.sources ?? [];
  return inProject(items, filter.projectId)
    .filter((item) => inScope(item, filter.scope, now))
    .filter((item) => chips.every((chip) => matchesChip(item, chip)))
    .filter((item) => sources.length === 0 || sources.includes(item.source))
    .filter(
      (item) =>
        !needle ||
        `${item.title} ${item.number ?? ""} ${item.repo} ${item.author.name}`
          .toLowerCase()
          .includes(needle),
    )
    .sort((a, b) => b.updatedAt - a.updatedAt);
}

/** The sources in the list, in the order first seen, for the source chips. */
export function sourcesIn(items: InboxItem[]): InboxSource[] {
  return [...new Set(items.map((item) => item.source))];
}

/** Counts for badges and the switch, over open items in the project. */
export function countItems(items: InboxItem[], projectId: string | undefined, now: number) {
  const open = inProject(items, projectId).filter((item) => inScope(item, "all", now));
  return {
    unread: open.filter((item) => item.unread).length,
    needsMe: open.filter(needsMe).length,
  };
}

export type InboxGrouping = "day" | "project";
export type InboxGroup = { id: string; title: string; items: InboxItem[] };

/**
 * Items under headers: Today, Yesterday and Earlier by day, or one per
 * project, in the order the (already sorted) items first appear.
 */
export function groupItems(
  items: InboxItem[],
  by: InboxGrouping,
  now: number,
  projectName: (projectId: string | undefined, repo: string) => string = (_, repo) => repo,
): InboxGroup[] {
  if (by === "project") {
    const groups = new Map<string, InboxGroup>();
    for (const item of items) {
      const id = item.projectId ?? `repo:${item.repo}`;
      const group = groups.get(id) ?? {
        id,
        title: projectName(item.projectId, item.repo),
        items: [],
      };
      group.items.push(item);
      groups.set(id, group);
    }
    return [...groups.values()];
  }
  const today = new Date(now).setHours(0, 0, 0, 0);
  const buckets: InboxGroup[] = [
    { id: "today", title: "Today", items: [] },
    { id: "yesterday", title: "Yesterday", items: [] },
    { id: "earlier", title: "Earlier", items: [] },
  ];
  for (const item of items) {
    const bucket =
      item.updatedAt >= today ? 0 : item.updatedAt >= today - DAY ? 1 : 2;
    buckets[bucket].items.push(item);
  }
  return buckets.filter((group) => group.items.length > 0);
}

// -- Marks ------------------------------------------------------------------

/** An item as a mark would leave it, shown at once before the desktop confirms. */
export function applyMark(
  item: InboxItem,
  action: InboxMarkAction,
  until?: number,
): InboxItem {
  switch (action) {
    case "seen":
      return { ...item, unread: false };
    case "unseen":
      return { ...item, unread: true };
    case "done":
      return { ...item, done: true, unread: false };
    case "undone":
      return { ...item, done: false };
    case "snooze":
      return { ...item, snoozedUntil: until, unread: false };
    case "unsnooze":
      return withoutSnooze(item);
  }
}

function withoutSnooze(item: InboxItem): InboxItem {
  const { snoozedUntil: _, ...rest } = item;
  return rest;
}

export type Mark = { action: InboxMarkAction; until?: number };

/**
 * The marks that take `from` back to how `to` was: what Undo sends. Only
 * what differs is sent, so undoing a Done on an unread item restores both
 * its place and its dot.
 */
export function marksToRestore(from: InboxItem, to: InboxItem): Mark[] {
  const marks: Mark[] = [];
  if (from.done !== to.done) marks.push({ action: to.done ? "done" : "undone" });
  if (from.snoozedUntil !== to.snoozedUntil) {
    marks.push(
      to.snoozedUntil === undefined
        ? { action: "unsnooze" }
        : { action: "snooze", until: to.snoozedUntil },
    );
  }
  if (from.unread !== to.unread) marks.push({ action: to.unread ? "unseen" : "seen" });
  return marks;
}

/** A mark's words in the toast. */
export function markMessage(action: InboxMarkAction) {
  switch (action) {
    case "done":
      return "Marked done";
    case "snooze":
      return "Snoozed";
    case "undone":
      return "Moved back to the inbox";
    case "unsnooze":
      return "Snooze cleared";
    case "seen":
      return "Marked as read";
    case "unseen":
      return "Marked as unread";
  }
}

/** Snooze choices: tomorrow at 9:00 and later today, as local times. */
export function snoozeOptions(now: number): { id: string; label: string; until: number }[] {
  const date = new Date(now);
  const later = new Date(now + 3 * 60 * 60 * 1000);
  const tomorrow = new Date(date);
  tomorrow.setDate(date.getDate() + 1);
  tomorrow.setHours(9, 0, 0, 0);
  const week = new Date(date);
  // The coming Monday, at 9:00.
  week.setDate(date.getDate() + (((8 - date.getDay()) % 7) || 7));
  week.setHours(9, 0, 0, 0);
  return [
    { id: "later", label: "Later today", until: later.getTime() },
    { id: "tomorrow", label: "Tomorrow, 9:00", until: tomorrow.getTime() },
    { id: "week", label: "Next week", until: week.getTime() },
  ];
}

// -- Optimistic state -------------------------------------------------------

/** The last list from the desktop and the marks it hasn't confirmed yet. */
export type MarkState = {
  server: InboxItem[];
  /** Items as a pending mark leaves them, by key; they win over `server`. */
  overrides: Record<string, InboxItem>;
};

export type MarkEvent =
  | { type: "server"; items: InboxItem[] }
  | { type: "optimistic"; item: InboxItem }
  | { type: "settled"; item: InboxItem }
  | { type: "rolledBack"; key: string };

export function markReducer(state: MarkState, event: MarkEvent): MarkState {
  switch (event.type) {
    case "server":
      // A pending mark outlasts a list that was read before the desktop heard of it.
      return { ...state, server: event.items };
    case "optimistic":
      return {
        ...state,
        overrides: { ...state.overrides, [event.item.key]: event.item },
      };
    case "settled": {
      const { [event.item.key]: _, ...overrides } = state.overrides;
      return {
        server: state.server.map((item) => (item.key === event.item.key ? event.item : item)),
        overrides,
      };
    }
    case "rolledBack": {
      const { [event.key]: _, ...overrides } = state.overrides;
      return { ...state, overrides };
    }
  }
}

/** The list with pending marks laid over it. */
export function withOverrides(state: MarkState): InboxItem[] {
  return state.server.map((item) => state.overrides[item.key] ?? item);
}

// -- Cache and staleness ----------------------------------------------------

/** What is kept on the phone of a list, to show before the desktop answers. */
export type CachedPage = Pick<
  InboxList,
  "items" | "nextCursor" | "fetchedAt" | "providers" | "counts"
>;

/** Reads a saved page, or null when it is missing or not a page. */
export function parseCachedPage(text: string | null | undefined): CachedPage | null {
  if (!text) return null;
  try {
    const page = JSON.parse(text) as Partial<CachedPage>;
    if (!page || !Array.isArray(page.items) || typeof page.fetchedAt !== "number") return null;
    return {
      items: page.items,
      nextCursor: page.nextCursor,
      fetchedAt: page.fetchedAt,
      providers: Array.isArray(page.providers) ? page.providers : [],
      counts: page.counts ?? { unread: 0, needsMe: 0, done: 0 },
    };
  } catch {
    return null;
  }
}

/** Past this a list read from the phone's cache is called out as old, offline or not. */
export const OLD_AFTER_MS = 30 * 60 * 1000;

export type InboxBanner =
  | { kind: "offline"; fetchedAt: number | null }
  | { kind: "stale"; fetchedAt: number; failing: string[] }
  | null;

/**
 * The note above the list. Offline wins, then a list the desktop itself calls
 * stale (a provider failed, or its snapshot is old). A list that hasn't
 * loaded has none: its skeleton says enough.
 */
export function bannerFor(feed: {
  loaded: boolean;
  offline: boolean;
  stale: boolean;
  fetchedAt: number | null;
  providers: { id: string; connected: boolean; error?: string }[];
}): InboxBanner {
  if (feed.offline && feed.loaded) return { kind: "offline", fetchedAt: feed.fetchedAt };
  if (feed.offline) return { kind: "offline", fetchedAt: null };
  if (!feed.loaded || feed.fetchedAt === null) return null;
  const failing = feed.providers
    .filter((provider) => !provider.connected || provider.error)
    .map((provider) => provider.id);
  if (feed.stale || failing.length > 0) {
    return { kind: "stale", fetchedAt: feed.fetchedAt, failing };
  }
  return null;
}

/** The key a list is cached and polled by. */
export function feedKey(query: {
  scope: InboxScope;
  projectId?: string;
  source?: InboxSource;
  q?: string;
}) {
  return [query.scope, query.projectId ?? "*", query.source ?? "*", query.q ?? ""].join("|");
}
