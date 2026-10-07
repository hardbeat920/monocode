// @vitest-environment happy-dom
import { beforeEach, describe, expect, it } from "vitest";
import {
  InboxTransitionTracker,
  resolveMissingTransitions,
} from "./inboxTransitions";
import type { InboxItem } from "./githubTasks";

function issue(overrides: Partial<InboxItem> = {}): InboxItem {
  return {
    provider: "github",
    kind: "issue",
    repo: "acme/web",
    number: 7,
    title: "Checkout fails",
    url: "https://github.com/acme/web/issues/7",
    updatedAt: "2026-10-01T08:00:00Z",
    state: "open",
    labels: [],
    assignees: [],
    draft: false,
    projectPath: "/tmp/web",
    ...overrides,
  };
}

function pr(overrides: Partial<InboxItem> = {}): InboxItem {
  return issue({
    kind: "pr",
    number: 9,
    url: "https://github.com/acme/web/pull/9",
    ...overrides,
  });
}

beforeEach(() => localStorage.clear());

const QUIET = { transitions: [], missing: [] };

describe("inbox transition tracker", () => {
  it("stays quiet the first time it sees an item, whatever its state", () => {
    const tracker = new InboxTransitionTracker();
    expect(
      tracker.observe(
        [issue(), issue({ number: 8, state: "closed" }), pr({ draft: true })],
        "all",
      ),
    ).toEqual(QUIET);
  });

  it("reports an issue closing and then reopening when closed items stay listed", () => {
    const tracker = new InboxTransitionTracker();
    tracker.observe([issue()], "all");
    const closed = issue({
      state: "closed",
      updatedAt: "2026-10-01T09:00:00Z",
    });
    expect(tracker.observe([closed], "all")).toEqual({
      transitions: [{ item: closed, transition: "closed" }],
      missing: [],
    });
    expect(tracker.observe([closed], "all")).toEqual(QUIET);
    const reopened = issue({ updatedAt: "2026-10-01T10:00:00Z" });
    expect(tracker.observe([reopened], "all")).toEqual({
      transitions: [{ item: reopened, transition: "reopened" }],
      missing: [],
    });
    expect(tracker.observe([reopened], "all")).toEqual(QUIET);
  });

  it("reports a merged pull request as merged rather than closed", () => {
    const tracker = new InboxTransitionTracker();
    tracker.observe([pr()], "all");
    const merged = pr({ state: "merged" });
    expect(tracker.observe([merged], "all").transitions).toEqual([
      { item: merged, transition: "merged" },
    ]);
  });

  it("reports a draft marked ready for review once", () => {
    const tracker = new InboxTransitionTracker();
    tracker.observe([pr({ draft: true })], "open");
    const ready = pr({ draft: false });
    expect(tracker.observe([ready], "open").transitions).toEqual([
      { item: ready, transition: "ready_for_review" },
    ]);
    expect(tracker.observe([ready], "open")).toEqual(QUIET);
  });

  it("does not call a draft ready when it was closed instead", () => {
    const tracker = new InboxTransitionTracker();
    tracker.observe([pr({ draft: true })], "all");
    const closed = pr({ draft: false, state: "closed" });
    expect(tracker.observe([closed], "all").transitions).toEqual([
      { item: closed, transition: "closed" },
    ]);
  });

  it("asks for a lookup when an open item leaves the list, and reports it closed once confirmed", () => {
    const tracker = new InboxTransitionTracker();
    const open = issue();
    tracker.observe([open], "open");
    expect(tracker.observe([], "open")).toEqual({
      transitions: [],
      missing: [open],
    });
    const closed = issue({
      state: "closed",
      updatedAt: "2026-10-01T09:00:00Z",
    });
    expect(tracker.resolve(closed)).toEqual([
      {
        item: closed,
        transition: "closed",
      },
    ]);
    expect(tracker.observe([], "open")).toEqual(QUIET);
  });

  it("treats an item that only dropped off the list as still open", () => {
    const tracker = new InboxTransitionTracker();
    const open = issue();
    tracker.observe([open], "open");
    tracker.observe([], "open");
    expect(tracker.resolve(open)).toEqual([]);
    expect(tracker.observe([], "open")).toEqual(QUIET);
    expect(tracker.observe([open], "open")).toEqual(QUIET);
  });

  it("reports reopened when a confirmed-closed item returns to the open list", () => {
    const tracker = new InboxTransitionTracker();
    tracker.observe([issue()], "open");
    tracker.observe([], "open");
    tracker.resolve(issue({ state: "closed" }));
    const reopened = issue({ updatedAt: "2026-10-01T11:00:00Z" });
    expect(tracker.observe([reopened], "open").transitions).toEqual([
      { item: reopened, transition: "reopened" },
    ]);
  });

  it("keeps asking while a lookup fails, then gives up after three failures", () => {
    const tracker = new InboxTransitionTracker();
    const open = issue();
    tracker.observe([open], "open");
    for (let attempt = 0; attempt < 3; attempt += 1) {
      expect(tracker.observe([], "open").missing).toEqual([open]);
      tracker.unresolved(open);
    }
    expect(tracker.observe([], "open")).toEqual(QUIET);
  });

  it("does not ask for lookups when the provider refresh failed", () => {
    const tracker = new InboxTransitionTracker();
    const open = issue();
    tracker.observe([open], "open");
    expect(tracker.observe([], "open", ["github"])).toEqual(QUIET);
    expect(tracker.observe([], "open").missing).toEqual([open]);
  });

  it("does not ask for lookups for items hidden by a changed query", () => {
    const tracker = new InboxTransitionTracker();
    tracker.observe([issue(), pr()], "0:open:/tmp/web");
    expect(tracker.observe([pr()], "1:open:/tmp/web")).toEqual(QUIET);
    expect(tracker.observe([pr()], "1:open:/tmp/web")).toEqual(QUIET);
  });

  it("still reports a real state change seen across a changed query", () => {
    const tracker = new InboxTransitionTracker();
    tracker.observe([issue()], "0:open:/tmp/web");
    const closed = issue({ state: "closed" });
    expect(tracker.observe([closed], "0:all:/tmp/web").transitions).toEqual([
      { item: closed, transition: "closed" },
    ]);
  });

  it("tracks the same number separately per kind and project", () => {
    const tracker = new InboxTransitionTracker();
    const other = issue({ repo: "acme/api", projectPath: "/tmp/api" });
    tracker.observe([issue(), pr({ number: 7 }), other], "all");
    const closed = issue({ state: "closed" });
    expect(
      tracker.observe([closed, pr({ number: 7 }), other], "all").transitions,
    ).toEqual([{ item: closed, transition: "closed" }]);
  });

  it("ignores providers other than GitHub", () => {
    const tracker = new InboxTransitionTracker();
    const gitlab = issue({ provider: "gitlab" });
    tracker.observe([gitlab], "all");
    expect(tracker.observe([{ ...gitlab, state: "closed" }], "all")).toEqual(
      QUIET,
    );
    expect(tracker.observe([], "all")).toEqual(QUIET);
  });
});

describe("resolving missing inbox items", () => {
  it("looks up each missing item and returns the confirmed transitions", async () => {
    const tracker = new InboxTransitionTracker();
    const a = issue();
    const b = pr();
    tracker.observe([a, b], "open");
    const { missing } = tracker.observe([], "open");
    const transitions = await resolveMissingTransitions(
      tracker,
      missing,
      async (item) => ({
        ...item,
        kind: item.kind === "pr" ? "pr" : "issue",
        state: item.kind === "pr" ? "merged" : "open",
      }),
    );
    expect(transitions).toEqual([
      { item: { ...b, state: "merged" }, transition: "merged" },
    ]);
    expect(tracker.observe([], "open")).toEqual(QUIET);
  });

  it("leaves an item for the next poll when its lookup fails", async () => {
    const tracker = new InboxTransitionTracker();
    const open = issue();
    tracker.observe([open], "open");
    const { missing } = tracker.observe([], "open");
    expect(
      await resolveMissingTransitions(tracker, missing, async () => {
        throw new Error("gh is offline");
      }),
    ).toEqual([]);
    expect(tracker.observe([], "open").missing).toEqual([open]);
  });

  it("ignores a lookup that completes after a newer observation", async () => {
    const tracker = new InboxTransitionTracker();
    const open = issue();
    tracker.observe([open], "open");
    const { missing } = tracker.observe([], "open");
    let finish!: (item: InboxItem) => void;
    const late = resolveMissingTransitions(
      tracker,
      missing,
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    tracker.observe([open], "open");
    finish(issue({ state: "closed" }));
    expect(await late).toEqual([]);
    expect(tracker.observe([open], "open").transitions).toEqual([]);
  });

  it("caps lookups per poll and leaves the rest for later", async () => {
    const tracker = new InboxTransitionTracker();
    const items = [1, 2, 3, 4].map((number) => issue({ number }));
    tracker.observe(items, "open");
    const { missing } = tracker.observe([], "open");
    const looked: number[] = [];
    await resolveMissingTransitions(
      tracker,
      missing,
      async (item) => {
        looked.push(item.number);
        return { ...item, kind: "issue", state: "closed" };
      },
      3,
    );
    expect(looked).toEqual([1, 2, 3]);
    expect(tracker.observe([], "open").missing).toEqual([items[3]]);
  });
});

describe("PR head observations", () => {
  const a = pr({ headRefOid: "a".repeat(40) });
  const b = pr({ headRefOid: "b".repeat(40) });

  it("baselines first sightings and reports changed/force-pushed heads independently of timestamps", () => {
    const tracker = new InboxTransitionTracker();
    expect(tracker.observe([a], "open")).toEqual(QUIET);
    expect(
      tracker.observe(
        [
          {
            ...a,
            title: "Edited",
            labels: [{ name: "bug", color: "fff" }],
            updatedAt: "2026-10-02T00:00:00Z",
          },
        ],
        "open",
      ),
    ).toEqual(QUIET);
    expect(tracker.observe([b], "open").transitions).toEqual([
      { item: b, transition: "head_changed", previousHead: a.headRefOid },
    ]);
    expect(tracker.observe([b], "open")).toEqual(QUIET);
    expect(tracker.observe([a], "open").transitions[0]?.previousHead).toBe(
      b.headRefOid,
    );
  });

  it("persists only head baselines and catches up to the latest head after restart", () => {
    const tracker = new InboxTransitionTracker();
    tracker.observe([a], "open");
    tracker.checkpoint();
    const restarted = new InboxTransitionTracker();
    expect(restarted.observe([a], "open")).toEqual(QUIET);
    expect(restarted.observe([b], "open", ["github"])).toEqual(QUIET);
    expect(restarted.observe([b], "open").transitions[0]).toEqual({
      item: b,
      transition: "head_changed",
      previousHead: a.headRefOid,
    });
    restarted.checkpoint();
    expect(new InboxTransitionTracker().observe([b], "open")).toEqual(QUIET);
  });

  it("does not advance the durable baseline before the queue handoff", () => {
    const tracker = new InboxTransitionTracker();
    tracker.observe([a], "open");
    tracker.checkpoint();
    tracker.observe([b], "open");
    expect(
      new InboxTransitionTracker().observe([b], "open").transitions,
    ).toHaveLength(1);
  });

  it("keeps repository identity and ignores missing heads, issues, and closed PR heads", () => {
    const tracker = new InboxTransitionTracker();
    tracker.observe([a, { ...a, repo: "acme/other" }], "all");
    expect(
      tracker.observe([{ ...b, repo: "acme/other" }, a], "all").transitions,
    ).toHaveLength(1);
    expect(
      tracker
        .observe([{ ...b, state: "merged" }], "all")
        .transitions.map((t) => t.transition),
    ).toEqual(["merged"]);
    expect(
      new InboxTransitionTracker().observe(
        [issue({ headRefOid: b.headRefOid })],
        "all",
      ).transitions,
    ).toEqual([]);
    const missing = new InboxTransitionTracker();
    missing.observe([pr()], "open");
    expect(missing.observe([a], "open")).toEqual(QUIET);
  });

  it("reports a ready transition alongside the changed head, including bounded lookups", () => {
    const tracker = new InboxTransitionTracker();
    tracker.observe([{ ...a, draft: true }], "open");
    expect(tracker.resolve(b).map((t) => t.transition)).toEqual([
      "ready_for_review",
      "head_changed",
    ]);
  });
});
