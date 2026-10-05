import { afterEach, expect, it } from "vitest";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { ChangeFeed } from "./changes";
import { HostStore } from "./store";

const cleanups: Array<() => void> = [];
afterEach(() => {
  for (const cleanup of cleanups.splice(0)) cleanup();
});

it("resets a desktop from another host run, then reports only newer writes", () => {
  const feed = new ChangeFeed();
  expect(feed.read(undefined, 0)).toEqual({
    boot: feed.boot,
    cursor: 0,
    sessions: [],
    reset: true,
  });
  feed.record({ id: "a", projectId: "p", revision: 1 });
  feed.record({ id: "b", projectId: "p", revision: 1 });
  feed.record({ id: "a", projectId: "p", revision: 2 });
  expect(feed.read(feed.boot, 1)).toEqual({
    boot: feed.boot,
    cursor: 3,
    sessions: [
      { id: "b", projectId: "p", revision: 1 },
      { id: "a", projectId: "p", revision: 2 },
    ],
    reset: false,
  });
  expect(feed.read(feed.boot, 3).sessions).toEqual([]);
  expect(feed.read("other boot", 3).reset).toBe(true);
  expect(feed.read(feed.boot, 4).reset).toBe(true);
});

it("resets a cursor older than the kept log", () => {
  const feed = new ChangeFeed();
  for (let revision = 1; revision <= 2_100; revision++)
    feed.record({ id: "a", projectId: "p", revision });
  expect(feed.read(feed.boot, 10).reset).toBe(true);
  expect(feed.read(feed.boot, 2_000).sessions).toEqual([
    { id: "a", projectId: "p", revision: 2_100 },
  ]);
});

it("wakes a waiting request once a burst of writes settles", async () => {
  const feed = new ChangeFeed();
  const started = Date.now();
  const waiting = feed.wait(feed.boot, 0, 5_000);
  feed.record({ id: "a", projectId: "p", revision: 1 });
  setTimeout(() => feed.record({ id: "a", projectId: "p", revision: 2 }), 10);
  const result = await waiting;
  expect(Date.now() - started).toBeLessThan(1_000);
  expect(result.sessions).toEqual([{ id: "a", projectId: "p", revision: 2 }]);
});

it("returns no changes at the timeout or when the desktop leaves", async () => {
  const feed = new ChangeFeed();
  expect((await feed.wait(feed.boot, 0, 20)).sessions).toEqual([]);
  const controller = new AbortController();
  const waiting = feed.wait(feed.boot, 0, 20_000, controller.signal);
  controller.abort();
  expect(await waiting).toMatchObject({ reset: false, sessions: [] });
});

it("records saves and deletions from the store", () => {
  const directory = mkdtempSync(join(tmpdir(), "monocode-changes-"));
  const store = new HostStore(join(directory, "host.db"));
  cleanups.push(() => {
    store.close();
    rmSync(directory, { recursive: true, force: true });
  });
  const project = store.addProject(directory, "demo");
  store.save(
    {
      projectId: project.id,
      revision: 1,
      status: "idle",
      updatedAt: 1,
      session: {
        id: "s1",
        title: "Demo",
        harness: "codex",
        model: "codex:test",
        busy: false,
        blocks: [],
      } as never,
    },
    { type: "test" },
  );
  store.deleteSession("s1");
  expect(store.changes.read(store.changes.boot, 0).sessions).toEqual([
    {
      id: "s1",
      projectId: project.id,
      revision: 1,
      deleted: true,
      status: "idle",
      busy: false,
    },
  ]);
});

it("exchanges a pairing code once, before it expires", () => {
  const directory = mkdtempSync(join(tmpdir(), "monocode-pairing-"));
  const store = new HostStore(join(directory, "host.db"));
  cleanups.push(() => {
    store.close();
    rmSync(directory, { recursive: true, force: true });
  });
  const now = 1_000_000;
  const { code, expiresAt } = store.issuePairing(now);
  expect(code).toMatch(/^[\w-]{43}$/);
  expect(store.pendingPairings(now)).toBe(1);
  expect(store.redeemPairing("wrong", "Laptop", now)).toBeUndefined();
  const device = store.redeemPairing(code, "Laptop", now)!;
  expect(store.authenticated(device.token)).toBe(true);
  expect(store.devices()).toEqual([{ id: device.id, name: "Laptop" }]);
  expect(store.redeemPairing(code, "Again", now)).toBeUndefined();
  const late = store.issuePairing(now);
  expect(store.redeemPairing(late.code, "Late", expiresAt)).toBeUndefined();
  expect(store.pendingPairings(expiresAt)).toBe(0);
});
