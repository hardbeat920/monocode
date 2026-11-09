import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createFixtureClient } from "@/features/inbox/fixtures";
import { createInboxStore, FOLLOWED_POLL_MS, type InboxStorage } from "@/features/inbox/store";
import {
  createInboxClient,
  type InboxClient,
  type InboxItem,
  type InboxList,
  type InboxListParams,
} from "@/features/inbox/protocol";

const NOW = new Date("2026-11-01T14:00:00").getTime();

function memory(): InboxStorage & { data: Map<string, string> } {
  const data = new Map<string, string>();
  return { data, get: (key) => data.get(key) ?? null, set: (key, value) => void data.set(key, value) };
}

function fixture(overrides: Partial<ReturnType<typeof createFixtureClient>["controls"]> = {}) {
  return createFixtureClient({ projectIds: ["p1", "p2", "p3"], now: () => NOW, controls: { latencyMs: 0, ...overrides } });
}

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

describe("following the desktop's Inbox rev", () => {
  const counted = () => {
    const client = fixture();
    const list = vi.spyOn(client, "list");
    const store = createInboxStore({ client, storage: memory(), namespace: "d", pollMs: 5000 });
    const stop = store.watch({ scope: "all", projectId: "p1" });
    return { list, store, stop };
  };

  it("reads when the rev moves and otherwise leaves the radio alone", async () => {
    const { list, store, stop } = counted();
    store.follow("a");
    await vi.advanceTimersByTimeAsync(0);
    const reads = list.mock.calls.length;
    await vi.advanceTimersByTimeAsync(60_000);
    expect(list.mock.calls.length).toBe(reads);
    store.follow("a");
    await vi.advanceTimersByTimeAsync(0);
    expect(list.mock.calls.length).toBe(reads);
    store.follow("b");
    await vi.advanceTimersByTimeAsync(0);
    expect(list.mock.calls.length).toBe(reads + 1);
    stop();
  });

  it("still reads now and then, for what the rev can't see", async () => {
    const { list, store, stop } = counted();
    store.follow("a");
    await vi.advanceTimersByTimeAsync(0);
    const reads = list.mock.calls.length;
    await vi.advanceTimersByTimeAsync(FOLLOWED_POLL_MS);
    expect(list.mock.calls.length).toBe(reads + 1);
    stop();
  });

  it("polls as before on a desktop without one", async () => {
    const { list, store, stop } = counted();
    store.follow(null);
    await vi.advanceTimersByTimeAsync(0);
    const reads = list.mock.calls.length;
    await vi.advanceTimersByTimeAsync(10_000);
    expect(list.mock.calls.length).toBe(reads + 2);
    stop();
  });
});

describe("the store", () => {
  it("reads a list, caches its first page and serves the cache to a new store", async () => {
    const storage = memory();
    const store = createInboxStore({ client: fixture(), storage, namespace: "desk" });
    const query = { scope: "all" as const, projectId: "p1" };
    expect(store.getFeed(query).loaded).toBe(false);
    await store.refresh(query);
    const feed = store.getFeed(query);
    expect(feed.loaded).toBe(true);
    expect(feed.items.length).toBeGreaterThan(0);
    expect(feed.items.every((item) => item.projectId === "p1")).toBe(true);
    expect(feed.fetchedAt).toBe(NOW);

    // A second launch shows the saved page before any answer.
    const again = createInboxStore({ client: fixture({ offline: true }), storage, namespace: "desk" });
    const cached = again.getFeed(query);
    expect(cached.loaded).toBe(true);
    expect(cached.fromCache).toBe(true);
    expect(cached.items.map((item) => item.key)).toEqual(feed.items.map((item) => item.key));
  });

  it("asks the desktop for a filtered list and keeps it out of the cache", async () => {
    const storage = memory();
    const client = fixture();
    const list = vi.spyOn(client, "list");
    const store = createInboxStore({ client, storage, namespace: "desk" });
    const query = { scope: "all" as const, facets: { kind: "issue" as const } };
    await store.refresh(query);
    expect(list.mock.calls[0][0]).toMatchObject({ kind: "issue" });
    const feed = store.getFeed(query);
    expect(feed.items.length).toBeGreaterThan(0);
    expect(feed.items.every((item) => item.kind === "issue")).toBe(true);
    expect(feed.options?.authors.length).toBeGreaterThan(0);
    const again = createInboxStore({ client, storage, namespace: "desk" });
    expect(again.getFeed(query).loaded).toBe(false);
  });

  it("keeps the cache per desktop", async () => {
    const storage = memory();
    const store = createInboxStore({ client: fixture(), storage, namespace: "one" });
    await store.refresh({ scope: "all" });
    const other = createInboxStore({ client: fixture(), storage, namespace: "two" });
    expect(other.getFeed({ scope: "all" }).loaded).toBe(false);
  });

  it("goes offline but keeps what it had when the desktop can't be reached", async () => {
    const client = fixture();
    const store = createInboxStore({ client, storage: memory(), namespace: "d" });
    await store.refresh({ scope: "all" });
    client.controls.offline = true;
    await store.refresh({ scope: "all" });
    const feed = store.getFeed({ scope: "all" });
    expect(feed.offline).toBe(true);
    expect(feed.items.length).toBeGreaterThan(0);
    client.controls.offline = false;
    await store.refresh({ scope: "all" });
    expect(store.getFeed({ scope: "all" }).offline).toBe(false);
  });

  it("makes no requests while the desktop isn't linked", async () => {
    const client = fixture();
    const list = vi.spyOn(client, "list");
    const store = createInboxStore({ client, storage: memory(), namespace: "d" });
    store.setLinked(false);
    await store.refresh({ scope: "all" });
    expect(list).not.toHaveBeenCalled();
    expect(store.getFeed({ scope: "all" }).offline).toBe(true);
  });

  it("polls a watched list on the catalog's cadence, only while the app is active", async () => {
    const client = fixture();
    const list = vi.spyOn(client, "list");
    let active = true;
    const store = createInboxStore({ client, storage: memory(), namespace: "d", pollMs: 5000, isActive: () => active });
    const stop = store.watch({ scope: "all" });
    await vi.advanceTimersByTimeAsync(0);
    expect(list).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(5000);
    expect(list).toHaveBeenCalledTimes(2);
    active = false;
    await vi.advanceTimersByTimeAsync(10_000);
    expect(list).toHaveBeenCalledTimes(2);
    active = true;
    await vi.advanceTimersByTimeAsync(5000);
    expect(list).toHaveBeenCalledTimes(3);
    stop();
    await vi.advanceTimersByTimeAsync(20_000);
    expect(list).toHaveBeenCalledTimes(3);
  });

  it("shares one watch between screens that ask for the same list", async () => {
    const client = fixture();
    const list = vi.spyOn(client, "list");
    const store = createInboxStore({ client, storage: memory(), namespace: "d", pollMs: 5000 });
    const a = store.watch({ scope: "all" });
    const b = store.watch({ scope: "all" });
    await vi.advanceTimersByTimeAsync(0);
    expect(list).toHaveBeenCalledTimes(1);
    a();
    await vi.advanceTimersByTimeAsync(5000);
    expect(list).toHaveBeenCalledTimes(2);
    b();
  });

  it("pages in more with the cursor without duplicates", async () => {
    const client = fixture();
    const real = client.list;
    client.list = (params) => real({ ...params, limit: 2 });
    const store = createInboxStore({ client, storage: memory(), namespace: "d" });
    await store.refresh({ scope: "all" });
    const first = store.getFeed({ scope: "all" });
    expect(first.items).toHaveLength(2);
    expect(first.nextCursor).toBeDefined();
    await store.loadMore({ scope: "all" });
    const next = store.getFeed({ scope: "all" });
    expect(next.items.length).toBeGreaterThan(2);
    expect(new Set(next.items.map((item) => item.key)).size).toBe(next.items.length);
  });

  it("returns the same feed object until something changes", async () => {
    const store = createInboxStore({ client: fixture(), storage: memory(), namespace: "d" });
    await store.refresh({ scope: "all" });
    expect(store.getFeed({ scope: "all" })).toBe(store.getFeed({ scope: "all" }));
  });
});

describe("a read that changes nothing", () => {
  /** A desktop whose every reply arrives as new objects, as over the link. */
  function linked() {
    const client = fixture();
    const list = client.list;
    return {
      ...client,
      list: (params: InboxListParams) =>
        list(params).then((page) => ({
          ...(JSON.parse(JSON.stringify(page)) as InboxList),
          fetchedAt: NOW,
        })),
    };
  }
  const query = { scope: "all" as const };

  it("leaves the feed as the same object, during the read and after it", async () => {
    const store = createInboxStore({ client: linked(), storage: memory(), namespace: "d" });
    await store.refresh(query);
    const feed = store.getFeed(query);
    const reading = store.refresh(query);
    expect(store.getFeed(query)).toBe(feed);
    await reading;
    expect(store.getFeed(query)).toBe(feed);
  });

  it("gives only the item that changed a new object", async () => {
    const client = linked();
    const store = createInboxStore({ client, storage: memory(), namespace: "d" });
    await store.refresh(query);
    const [one, two, ...rest] = store.getFeed(query).items;
    const list = client.list;
    client.list = (params) =>
      list(params).then((page) => ({
        ...page,
        items: page.items.map((item) =>
          item.key === one.key ? { ...item, title: "Renamed" } : item,
        ),
      }));
    await store.refresh(query);
    const items = store.getFeed(query).items;
    expect(items[0]).not.toBe(one);
    expect(items[0].title).toBe("Renamed");
    expect(items[1]).toBe(two);
    expect(items.slice(2)).toEqual(rest);
    expect(items[2]).toBe(rest[0]);
  });

  it("doesn't tell the screens or write the phone's cache", async () => {
    const storage = memory();
    const set = vi.spyOn(storage, "set");
    const store = createInboxStore({ client: linked(), storage, namespace: "d" });
    await store.refresh(query);
    const told = vi.fn();
    store.subscribe(told);
    set.mockClear();
    await store.refresh(query);
    expect(told).not.toHaveBeenCalled();
    expect(set).not.toHaveBeenCalled();
  });
});

describe("marks", () => {
  async function loaded(client: InboxClient = fixture()) {
    const store = createInboxStore({ client, storage: memory(), namespace: "d" });
    await store.refresh({ scope: "all" });
    return store;
  }
  const first = (store: Awaited<ReturnType<typeof loaded>>) => store.getFeed({ scope: "all" }).items.find((item) => item.unread)!;

  it("shows a mark before the desktop answers and keeps it after", async () => {
    const client = fixture({ latencyMs: 100 });
    const store = createInboxStore({ client, storage: memory(), namespace: "d" });
    const refreshing = store.refresh({ scope: "all" });
    await vi.advanceTimersByTimeAsync(100);
    await refreshing;
    const target = first(store);
    const marking = store.mark(target.key, "done");
    expect(store.getFeed({ scope: "all" }).items.find((item) => item.key === target.key)?.done).toBe(true);
    await vi.advanceTimersByTimeAsync(100);
    await marking;
    expect(store.find(target.key)?.done).toBe(true);
  });

  it("rolls back when the desktop can't be reached", async () => {
    const client = fixture();
    const store = await loaded(client);
    const target = first(store);
    client.controls.offline = true;
    await expect(store.mark(target.key, "done")).rejects.toThrow();
    expect(store.find(target.key)).toEqual(target);
  });

  it("undoes a Done on an unread item with the marks that restore it", async () => {
    const client = fixture();
    const mark = vi.spyOn(client, "mark");
    const store = await loaded(client);
    const target = first(store);
    const { undo } = await store.mark(target.key, "done");
    mark.mockClear();
    await undo();
    expect(mark.mock.calls.map((call) => call[1])).toEqual(["undone", "unseen"]);
    expect(store.find(target.key)).toMatchObject({ done: false, unread: true });
  });

  it("undoes a snooze", async () => {
    const client = fixture();
    const store = await loaded(client);
    const target = first(store);
    const { undo } = await store.mark(target.key, "snooze", NOW + 3_600_000);
    expect(store.find(target.key)?.snoozedUntil).toBe(NOW + 3_600_000);
    await undo();
    expect(store.find(target.key)?.snoozedUntil).toBeUndefined();
  });

  it("refuses to mark an item it has never seen", async () => {
    const store = await loaded();
    await expect(store.mark("github:nope#1", "done")).rejects.toThrow();
  });
});

describe("the client", () => {
  it("sends the contract's methods and drops undefined params", async () => {
    const transport = vi.fn(async (method: string) => (method === "inbox.checks" ? { checks: [{ name: "lint", state: "passing" }] } : method === "inbox.mark" ? { item: { key: "k" } } : {}));
    const client = createInboxClient(transport as never, () => "id-1");
    await client.list({ scope: "needsMe", projectId: undefined, limit: 30 });
    expect(transport).toHaveBeenLastCalledWith("inbox.list", { scope: "needsMe", limit: 30 });
    await client.diff("k", undefined);
    expect(transport).toHaveBeenLastCalledWith("inbox.diff", { key: "k" });
    expect(await client.checks("k")).toEqual([{ name: "lint", state: "passing" }]);
    expect(await client.mark("k", "snooze", 5)).toEqual({ key: "k" });
    expect(transport).toHaveBeenLastCalledWith("inbox.mark", { key: "k", action: "snooze", until: 5 });
    await client.media("k", { avatarKey: "a" });
    expect(transport).toHaveBeenLastCalledWith("inbox.media", { key: "k", avatarKey: "a" });
    await client.media("k", { commentId: "c1", commentIndex: 2 });
    expect(transport).toHaveBeenLastCalledWith("inbox.media", {
      key: "k",
      commentId: "c1",
      commentIndex: 2,
    });
  });

  it("keeps one command id across a retried start and a fresh one after success", async () => {
    let n = 0;
    const transport = vi
      .fn()
      .mockRejectedValueOnce(new Error("lost"))
      .mockResolvedValue({ sessionId: "s" });
    const client = createInboxClient(transport, () => `id-${++n}`);
    const params = { key: "k", projectId: "p", mode: "work" as const };
    await expect(client.start(params)).rejects.toThrow();
    await client.start(params);
    expect(transport.mock.calls[0][2]).toBe("id-1");
    expect(transport.mock.calls[1][2]).toBe("id-1");
    await client.start(params);
    expect(transport.mock.calls[2][2]).toBe("id-2");
  });
});

describe("staying in step with the desktop", () => {
  const row = (n: number): InboxItem => ({
    key: `github:o/r#${n}`,
    source: "github",
    kind: "pr",
    title: `PR ${n}`,
    repo: "o/r",
    author: { name: "a" },
    status: "open",
    updatedAt: 1000 - n,
    unread: false,
    done: false,
    comments: 0,
    url: "https://example.test",
  });

  /** A desktop that pages `all` by 3 and records every request. */
  function desktop(count: number) {
    let all = Array.from({ length: count }, (_, i) => row(i));
    const calls: InboxListParams[] = [];
    const client = {
      list: async (params: InboxListParams) => {
        calls.push(params);
        const start = params.cursor ? Number(params.cursor) : 0;
        return {
          items: all.slice(start, start + 3),
          nextCursor: start + 3 < all.length ? String(start + 3) : undefined,
          stale: false,
          fetchedAt: NOW,
          providers: [],
          counts: { unread: 0, needsMe: 0, done: 0 },
        };
      },
    } as unknown as InboxClient;
    return {
      client,
      calls,
      drop: (n: number) => (all = all.filter((item) => item.key !== `github:o/r#${n}`)),
    };
  }
  const numbers = (store: ReturnType<typeof createInboxStore>, query: { scope: "all" }) =>
    store.getFeed(query).items.map((item) => Number(item.key.split("#")[1]));

  it("drops a scrolled-in item the desktop no longer lists, and keeps the depth", async () => {
    const { client, drop } = desktop(9);
    const store = createInboxStore({ client, storage: memory(), namespace: "d" });
    const query = { scope: "all" as const };
    await store.refresh(query);
    await store.loadMore(query);
    expect(numbers(store, query)).toEqual([0, 1, 2, 3, 4, 5]);
    drop(4);
    await store.refresh(query);
    // Re-read down to where the list was, so the scroll position holds.
    expect(numbers(store, query)).toEqual([0, 1, 2, 3, 5, 6]);
    expect(store.getFeed(query).nextCursor).toBeDefined();
  });

  it("asks the desktop to fetch on a forced refresh, not on a poll", async () => {
    const { client, calls } = desktop(3);
    const store = createInboxStore({ client, storage: memory(), namespace: "d" });
    const query = { scope: "all" as const };
    await store.refresh(query);
    await store.refresh(query, { force: true });
    expect(calls.map((call) => call.refresh)).toEqual([undefined, true]);
  });

  it("runs a forced refresh after a poll that is already on its way", async () => {
    const { client, calls } = desktop(3);
    const store = createInboxStore({ client, storage: memory(), namespace: "d" });
    const query = { scope: "all" as const };
    const poll = store.refresh(query);
    const pull = store.refresh(query, { force: true });
    await Promise.all([poll, pull]);
    expect(calls.map((call) => call.refresh)).toEqual([undefined, true]);
  });

  it("ignores a page that was asked for before a refresh replaced the list", async () => {
    const { client, drop } = desktop(9);
    const store = createInboxStore({ client, storage: memory(), namespace: "d" });
    const query = { scope: "all" as const };
    await store.refresh(query);
    const more = store.loadMore(query);
    drop(0);
    await store.refresh(query);
    await more;
    const items = numbers(store, query);
    expect(new Set(items).size).toBe(items.length);
    expect(items).not.toContain(0);
  });
});
