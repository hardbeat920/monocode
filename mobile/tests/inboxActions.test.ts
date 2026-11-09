import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createFixtureClient } from "@/features/inbox/fixtures";
import { actionCopy, actionMessage, mergeOptions } from "@/features/inbox/itemActions";
import { createInboxClient, type InboxTransport } from "@/features/inbox/protocol";
import { createInboxStore, type InboxStorage } from "@/features/inbox/store";

const NOW = new Date("2026-11-01T14:00:00").getTime();

function memory(): InboxStorage {
  const data = new Map<string, string>();
  return { get: (key) => data.get(key) ?? null, set: (key, value) => void data.set(key, value) };
}

function fixture() {
  return createFixtureClient({
    projectIds: ["p1", "p2", "p3"],
    now: () => NOW,
    controls: { latencyMs: 0 },
  });
}

/** A transport that loses the first `lost` replies, and the ids it was given. */
function lossy(lost: number) {
  const ids: (string | undefined)[] = [];
  const calls: { method: string; params?: object }[] = [];
  const transport: InboxTransport = async <T,>(method: string, params?: object, id?: string) => {
    ids.push(id);
    calls.push({ method, params });
    if (ids.length <= lost) throw new Error("The reply was lost");
    return { item: {}, actions: [], queued: false } as T;
  };
  let next = 0;
  return { transport, ids, calls, makeId: () => `id-${++next}` };
}

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

describe("commands that reach the provider", () => {
  it("retries a merge with the same id, so the desktop can't merge twice", async () => {
    const link = lossy(1);
    const client = createInboxClient(link.transport, link.makeId);
    await expect(client.act("k", "squash")).rejects.toThrow("lost");
    await client.act("k", "squash");
    expect(link.ids).toEqual(["id-1", "id-1"]);
  });

  it("retries a comment with the same id, so it can't post twice", async () => {
    const link = lossy(1);
    const client = createInboxClient(link.transport, link.makeId);
    await expect(client.comment("k", "Looks good")).rejects.toThrow("lost");
    await client.comment("k", "Looks good");
    expect(link.ids).toEqual(["id-1", "id-1"]);
  });

  it("gives a new id once the desktop answered, so the same words can be posted again", async () => {
    const link = lossy(0);
    const client = createInboxClient(link.transport, link.makeId);
    await client.comment("k", "+1");
    await client.comment("k", "+1");
    expect(link.ids).toEqual(["id-1", "id-2"]);
  });

  it("keeps different commands apart", async () => {
    const link = lossy(2);
    const client = createInboxClient(link.transport, link.makeId);
    await expect(client.act("k", "merge")).rejects.toThrow();
    await expect(client.act("k", "squash")).rejects.toThrow();
    expect(link.ids).toEqual(["id-1", "id-2"]);
  });

  it("sends a reply's target and leaves it out of a plain comment", async () => {
    const link = lossy(0);
    const client = createInboxClient(link.transport, link.makeId);
    await client.comment("k", "Hi");
    await client.comment("k", "Hi", "thread-1");
    expect(link.calls.map((call) => call.params)).toEqual([
      { key: "k", body: "Hi" },
      { key: "k", body: "Hi", replyTo: "thread-1" },
    ]);
  });
});

describe("the store", () => {
  it("shows a merged pull request in the list once the desktop says so", async () => {
    const client = fixture();
    const store = createInboxStore({ client, storage: memory(), namespace: "desk" });
    const query = { scope: "all" as const };
    await store.refresh(query);
    const pr = store
      .getFeed(query)
      .items.find((item) => item.source === "github" && item.kind === "pr" && item.status === "open");
    if (!pr) throw new Error("The fixtures have no open pull request");

    const result = await store.act(pr.key, "squash");

    expect(result.actions).toEqual(["comment", "review"]);
    expect(store.find(pr.key)?.status).toBe("merged");
    expect(store.getFeed(query).items.find((item) => item.key === pr.key)?.status).toBe("merged");
  });

  it("changes nothing when the desktop refuses an action", async () => {
    const client = fixture();
    const store = createInboxStore({ client, storage: memory(), namespace: "desk" });
    const query = { scope: "all" as const };
    await store.refresh(query);
    const issue = store.getFeed(query).items.find((item) => item.kind === "issue");
    if (!issue) throw new Error("The fixtures have no issue");

    await expect(store.act(issue.key, "merge")).rejects.toThrow("isn't available");

    expect(store.find(issue.key)).toEqual(issue);
  });

  it("marks a whole list read", async () => {
    const client = fixture();
    const store = createInboxStore({ client, storage: memory(), namespace: "desk" });
    const query = { scope: "all" as const };
    await store.refresh(query);
    expect(store.getFeed(query).items.some((item) => item.unread)).toBe(true);

    await store.readAll(query);

    const feed = store.getFeed(query);
    expect(feed.items.some((item) => item.unread)).toBe(false);
    expect(feed.counts.unread).toBe(0);
  });

  it("keeps a list unread when the desktop can't be reached", async () => {
    const client = fixture();
    const store = createInboxStore({ client, storage: memory(), namespace: "desk" });
    const query = { scope: "all" as const };
    await store.refresh(query);
    client.controls.offline = true;

    await expect(store.readAll(query)).rejects.toThrow();

    expect(store.getFeed(query).items.some((item) => item.unread)).toBe(true);
  });
});

describe("the words", () => {
  it("offers only the merges the desktop allows", () => {
    expect(mergeOptions(["comment", "merge", "squash", "rebase", "close"]).map((m) => m.action)).toEqual([
      "merge",
      "squash",
      "rebase",
    ]);
    expect(mergeOptions(["comment", "ready", "close"])).toEqual([]);
  });

  it("names the branches when it knows them", () => {
    expect(actionCopy("squash", { baseRef: "main", headRef: "fix/crash" }).detail).toBe(
      "The commits from “fix/crash” will be combined into one commit on “main”.",
    );
    expect(actionCopy("merge", {}).detail).toContain("this branch");
    expect(actionCopy("merge", {}).detail).toContain("the base branch");
  });

  it("says when a merge is only queued", () => {
    expect(actionMessage("squash", false)).toBe("Pull request merged");
    expect(actionMessage("squash", true)).toBe("Merge queued or auto-merge enabled");
    expect(actionMessage("close", false)).toBe("Pull request closed");
  });
});
