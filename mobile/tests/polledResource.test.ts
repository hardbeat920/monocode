import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const app = vi.hoisted(() => ({
  state: "active",
  listeners: new Set<(state: string) => void>(),
}));
vi.mock("react-native", () => ({
  AppState: {
    get currentState() {
      return app.state;
    },
    addEventListener: (_: string, listener: (state: string) => void) => {
      app.listeners.add(listener);
      return { remove: () => app.listeners.delete(listener) };
    },
  },
}));

import { PolledResource } from "@/features/connection/polledResource";
import { mergeChanges, sameFile, type Changes } from "@/features/connection/changed";
import { deepEqual } from "@/features/connection/deepEqual";
import { waitForActive, backoff } from "@/features/connection/appActivity";
import type { ChangedFile } from "@/features/workspace/types";

const setApp = (state: string) => {
  app.state = state;
  for (const listener of [...app.listeners]) listener(state);
};
const flush = () => vi.advanceTimersByTimeAsync(0);

beforeEach(() => {
  vi.useFakeTimers();
  app.state = "active";
  app.listeners.clear();
});
afterEach(() => vi.useRealTimers());

const same = (held: number | null, next: number) => (held === next ? held : next);

describe("PolledResource", () => {
  it("reads at once, then every base interval, and stops when released", async () => {
    const resource = new PolledResource<number>({ merge: same });
    const fetch = vi.fn(async () => 1);
    const release = resource.hold(fetch);
    await flush();
    expect(fetch).toHaveBeenCalledTimes(1);
    expect(resource.getSnapshot().value).toBe(1);
    await vi.advanceTimersByTimeAsync(10000);
    expect(fetch).toHaveBeenCalledTimes(2);
    release();
    await vi.advanceTimersByTimeAsync(120000);
    expect(fetch).toHaveBeenCalledTimes(2);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("shares one poller between everything holding it", async () => {
    const resource = new PolledResource<number>({ merge: same });
    const fetch = vi.fn(async () => 1);
    const first = resource.hold(fetch);
    const second = resource.hold(fetch);
    await vi.advanceTimersByTimeAsync(10000);
    expect(fetch).toHaveBeenCalledTimes(2);
    first();
    await vi.advanceTimersByTimeAsync(10000);
    expect(fetch).toHaveBeenCalledTimes(3);
    second();
  });

  it("does not read in the background and reads again when shown", async () => {
    const resource = new PolledResource<number>({ merge: same });
    const fetch = vi.fn(async () => 1);
    resource.hold(fetch);
    await flush();
    setApp("background");
    await vi.advanceTimersByTimeAsync(120000);
    expect(fetch).toHaveBeenCalledTimes(1);
    expect(vi.getTimerCount()).toBe(0);
    setApp("active");
    await flush();
    expect(fetch).toHaveBeenCalledTimes(2);
  });

  it("keeps reading every 10 s while nothing changes, as it always did", async () => {
    const resource = new PolledResource<number>({ merge: same });
    const fetch = vi.fn(async () => 1);
    resource.hold(fetch);
    await vi.advanceTimersByTimeAsync(300000);
    // 1 at once, then one every 10 s: no slowdown for a quiet working copy.
    expect(fetch.mock.calls.length).toBe(31);
  });

  it("reads at once when a screen takes hold again after being away, not after the rest of the interval", async () => {
    const resource = new PolledResource<number>({ merge: same });
    let value = 1;
    const fetch = vi.fn(async () => value);
    let release = resource.hold(fetch);
    await flush();
    // Focus leaves: nothing holds the poller. The working copy changes meanwhile.
    release();
    value = 2;
    await vi.advanceTimersByTimeAsync(3000);
    expect(resource.getSnapshot().value).toBe(1);
    // Focus returns three seconds into the interval: the badge is fresh straight away.
    release = resource.hold(fetch);
    await flush();
    expect(fetch).toHaveBeenCalledTimes(2);
    expect(resource.getSnapshot().value).toBe(2);
    // And the cadence goes on from that read.
    await vi.advanceTimersByTimeAsync(10000);
    expect(fetch).toHaveBeenCalledTimes(3);
  });

  it("does not read twice when two screens take hold together", async () => {
    const resource = new PolledResource<number>({ merge: same });
    const fetch = vi.fn(async () => 1);
    resource.hold(fetch);
    resource.hold(fetch);
    await flush();
    expect(fetch).toHaveBeenCalledTimes(1);
  });

  it("tells subscribers only about changes", async () => {
    const resource = new PolledResource<number>({ merge: same });
    const listener = vi.fn();
    resource.subscribe(listener);
    resource.hold(async () => 1);
    await vi.advanceTimersByTimeAsync(60000);
    expect(listener).toHaveBeenCalledTimes(1);
  });

  it("waits longer after failures and keeps the last value", async () => {
    const resource = new PolledResource<number>({ merge: same });
    let fail = false;
    const fetch = vi.fn(async () => {
      if (fail) throw new Error("offline");
      return 7;
    });
    resource.hold(fetch);
    await flush();
    fail = true;
    await vi.advanceTimersByTimeAsync(10000);
    expect(resource.getSnapshot()).toEqual({ value: 7, error: "offline" });
    const calls = fetch.mock.calls.length;
    await vi.advanceTimersByTimeAsync(60000);
    // 20 s, 40 s, 60 s waits: at most three more reads in a minute, not six.
    expect(fetch.mock.calls.length - calls).toBeLessThanOrEqual(3);
    fail = false;
    await vi.advanceTimersByTimeAsync(60000);
    expect(resource.getSnapshot()).toEqual({ value: 7, error: null });
  });

  it("reloads at once and reads again after an in-flight read", async () => {
    const resource = new PolledResource<number>({ merge: same });
    const fetch = vi.fn(async () => 1);
    resource.hold(fetch);
    await flush();
    resource.reload();
    await flush();
    expect(fetch).toHaveBeenCalledTimes(2);
  });

  it("holds the last read for the next screen to show at once", async () => {
    const resource = new PolledResource<number>({ merge: same });
    const release = resource.hold(async () => 5);
    await flush();
    release();
    expect(resource.getSnapshot().value).toBe(5);
  });
});

const file = (relative: string, text = "a", extra: Partial<ChangedFile> = {}): ChangedFile => ({
  relative,
  status: "modified",
  additions: 1,
  deletions: 0,
  lines: [{ kind: "add", text, oldNumber: null, newNumber: 1 }],
  ...extra,
});
const read = (...files: ChangedFile[]): Changes => ({ branch: "main", files });

describe("mergeChanges", () => {
  it("keeps the held read when nothing changed, without building strings", () => {
    const stringify = vi.spyOn(JSON, "stringify");
    const held = read(file("a.ts"), file("b.ts"));
    const next = read(file("a.ts"), file("b.ts"));
    expect(mergeChanges(held, next)).toBe(held);
    expect(stringify).not.toHaveBeenCalled();
    stringify.mockRestore();
  });

  it("keeps unchanged files and takes the changed one", () => {
    const held = read(file("a.ts"), file("b.ts"));
    const merged = mergeChanges(held, read(file("a.ts"), file("b.ts", "changed")));
    expect(merged).not.toBe(held);
    expect(merged.files[0]).toBe(held.files[0]);
    expect(merged.files[1].lines[0].text).toBe("changed");
  });

  it("notices an edit that leaves the counts alone, staging, a new file and a new branch", () => {
    expect(sameFile(file("a.ts"), file("a.ts", "b"))).toBe(false);
    expect(sameFile(file("a.ts"), file("a.ts", "a", { staged: true }))).toBe(false);
    const held = read(file("a.ts"));
    expect(mergeChanges(held, read(file("a.ts"), file("b.ts")))).not.toBe(held);
    expect(mergeChanges(held, { ...read(file("a.ts")), branch: "dev" })).not.toBe(held);
  });
});

describe("deepEqual", () => {
  it("compares structure and counts a missing field as undefined", () => {
    expect(deepEqual({ a: [1, { b: 2 }] }, { a: [1, { b: 2 }] })).toBe(true);
    expect(deepEqual({ a: 1, b: undefined }, { a: 1 })).toBe(true);
    expect(deepEqual({ a: 1 }, { a: 2 })).toBe(false);
    expect(deepEqual([1], [1, 2])).toBe(false);
    expect(deepEqual([], {})).toBe(false);
    expect(deepEqual(null, {})).toBe(false);
  });
});

describe("app activity", () => {
  it("waits for the next active event instead of polling", async () => {
    setApp("background");
    const done = vi.fn();
    void waitForActive().then(done);
    await vi.advanceTimersByTimeAsync(60000);
    expect(done).not.toHaveBeenCalled();
    expect(vi.getTimerCount()).toBe(0);
    setApp("active");
    await flush();
    expect(done).toHaveBeenCalled();
    expect(app.listeners.size).toBe(0);
  });

  it("lets an abort end the wait", async () => {
    setApp("background");
    const stop = new AbortController();
    const done = vi.fn();
    void waitForActive(stop.signal).then(done);
    stop.abort();
    await flush();
    expect(done).toHaveBeenCalled();
    expect(app.listeners.size).toBe(0);
  });

  it("doubles up to a ceiling", () => {
    expect([0, 1, 2, 5].map((n) => backoff(1000, n, 8000))).toEqual([1000, 2000, 4000, 8000]);
  });
});
