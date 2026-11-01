import { describe, expect, it } from "vitest";

import {
  applyMark,
  bannerFor,
  countItems,
  feedKey,
  groupItems,
  inProject,
  marksToRestore,
  markReducer,
  parseCachedPage,
  snoozeOptions,
  visibleItems,
  withOverrides,
  type MarkState,
} from "@/features/inbox/model";
import { checkLabel, toChangedFile } from "@/features/inbox/itemView";
import type { InboxItem } from "@/features/inbox/protocol";

const NOW = new Date("2026-11-01T14:00:00").getTime();
const HOUR = 3_600_000;

function item(over: Partial<InboxItem> & { key: string }): InboxItem {
  return {
    source: "github",
    kind: "pr",
    title: over.key,
    repo: "acme/app",
    author: { name: "sam" },
    status: "open",
    updatedAt: NOW - HOUR,
    unread: false,
    done: false,
    comments: 0,
    url: "https://example.com",
    ...over,
  };
}

const items = [
  item({ key: "a", attention: "reviewRequested", unread: true, projectId: "p1", updatedAt: NOW - HOUR }),
  item({ key: "b", attention: "other", projectId: "p1", updatedAt: NOW - 20 * HOUR }),
  item({ key: "c", kind: "issue", source: "linear", attention: "assigned", projectId: "p2", updatedAt: NOW - 40 * HOUR }),
  item({ key: "d", attention: "ciFailed", checks: "failing", projectId: "p1", unread: true, updatedAt: NOW - 2 * HOUR }),
  item({ key: "e", done: true, attention: "mentioned", projectId: "p1" }),
  item({ key: "f", snoozedUntil: NOW + HOUR, attention: "mentioned", projectId: "p1" }),
  item({ key: "g", projectId: undefined, attention: "mentioned", updatedAt: NOW - 3 * HOUR }),
];

const keys = (list: InboxItem[]) => list.map((entry) => entry.key);

describe("scope filtering", () => {
  it("All shows open, unsnoozed items, newest first", () => {
    expect(keys(visibleItems(items, { scope: "all" }, NOW))).toEqual(["a", "d", "g", "b", "c"]);
  });

  it("Needs me keeps review, mention, assignment and failing CI only", () => {
    expect(keys(visibleItems(items, { scope: "needsMe" }, NOW))).toEqual(["a", "d", "g", "c"]);
  });

  it("Done shows done items, even snoozed ones are not mixed in", () => {
    expect(keys(visibleItems(items, { scope: "done" }, NOW))).toEqual(["e"]);
  });

  it("a snooze that has passed brings the item back", () => {
    expect(keys(visibleItems(items, { scope: "all" }, NOW + 2 * HOUR))).toContain("f");
  });

  it("scopes to a project; unmapped items only appear with all projects", () => {
    expect(keys(inProject(items, "p1"))).not.toContain("g");
    expect(keys(inProject(items, undefined))).toContain("g");
    expect(keys(visibleItems(items, { scope: "all", projectId: "p2" }, NOW))).toEqual(["c"]);
  });

  it("applies chips, sources and search together", () => {
    expect(keys(visibleItems(items, { scope: "all", chips: ["unread"] }, NOW))).toEqual(["a", "d"]);
    expect(keys(visibleItems(items, { scope: "all", chips: ["ci"] }, NOW))).toEqual(["d"]);
    expect(keys(visibleItems(items, { scope: "all", chips: ["issues"] }, NOW))).toEqual(["c"]);
    expect(keys(visibleItems(items, { scope: "all", sources: ["linear"] }, NOW))).toEqual(["c"]);
    expect(keys(visibleItems(items, { scope: "all", query: " REVIEW " }, NOW))).toEqual([]);
    expect(keys(visibleItems([item({ key: "x", title: "Fix the backoff" })], { scope: "all", query: "backoff" }, NOW))).toEqual(["x"]);
  });

  it("counts unread and needs-me over open items in the project", () => {
    expect(countItems(items, "p1", NOW)).toEqual({ unread: 2, needsMe: 2 });
    expect(countItems(items, undefined, NOW)).toEqual({ unread: 2, needsMe: 4 });
  });
});

describe("grouping", () => {
  it("groups by day with the empty groups left out", () => {
    const sorted = visibleItems(items, { scope: "all" }, NOW);
    const groups = groupItems(sorted, "day", NOW);
    expect(groups.map((group) => group.title)).toEqual(["Today", "Yesterday", "Earlier"]);
    expect(keys(groups[0].items)).toEqual(["a", "d", "g"]);
    expect(keys(groups[1].items)).toEqual(["b"]);
    expect(keys(groups[2].items)).toEqual(["c"]);
    expect(groupItems([sorted[0]], "day", NOW).map((group) => group.id)).toEqual(["today"]);
  });

  it("groups by project in order of first appearance, naming unmapped items by repo", () => {
    const sorted = visibleItems(items, { scope: "all" }, NOW);
    const groups = groupItems(sorted, "project", NOW, (id, repo) => id ?? repo);
    expect(groups.map((group) => group.title)).toEqual(["p1", "acme/app", "p2"]);
    expect(keys(groups[0].items)).toEqual(["a", "d", "b"]);
  });
});

describe("marks", () => {
  const base = item({ key: "m", unread: true });

  it("applies each mark to the item", () => {
    expect(applyMark(base, "seen").unread).toBe(false);
    expect(applyMark({ ...base, unread: false }, "unseen").unread).toBe(true);
    expect(applyMark(base, "done")).toMatchObject({ done: true, unread: false });
    expect(applyMark({ ...base, done: true }, "undone").done).toBe(false);
    expect(applyMark(base, "snooze", 99).snoozedUntil).toBe(99);
    expect("snoozedUntil" in applyMark({ ...base, snoozedUntil: 5 }, "unsnooze")).toBe(false);
  });

  it("undoing a Done on an unread item restores its place and its dot", () => {
    const after = applyMark(base, "done");
    expect(marksToRestore(after, base)).toEqual([
      { action: "undone" },
      { action: "unseen" },
    ]);
  });

  it("undoing a snooze unsnoozes; undoing an unsnooze snoozes again", () => {
    const snoozed = applyMark(base, "snooze", 1234);
    expect(marksToRestore(snoozed, base)).toEqual([
      { action: "unsnooze" },
      { action: "unseen" },
    ]);
    expect(marksToRestore(base, snoozed)).toContainEqual({ action: "snooze", until: 1234 });
  });

  it("sends nothing when nothing differs", () => {
    expect(marksToRestore(base, base)).toEqual([]);
  });

  it("offers snooze times in the future", () => {
    const options = snoozeOptions(NOW);
    expect(options.map((option) => option.id)).toEqual(["later", "tomorrow", "week"]);
    expect(options.every((option) => option.until > NOW)).toBe(true);
    expect(new Date(options[1].until).getHours()).toBe(9);
    expect(new Date(options[2].until).getDay()).toBe(1);
  });
});

describe("mark reducer", () => {
  const start: MarkState = { server: [item({ key: "a", unread: true }), item({ key: "b" })], overrides: {} };

  it("shows a pending mark over the server list and keeps it across a read", () => {
    let state = markReducer(start, { type: "optimistic", item: applyMark(start.server[0], "done") });
    expect(withOverrides(state)[0].done).toBe(true);
    // A list read before the desktop heard of the mark doesn't undo it.
    state = markReducer(state, { type: "server", items: start.server });
    expect(withOverrides(state)[0].done).toBe(true);
  });

  it("settles on the desktop's reply", () => {
    const optimistic = applyMark(start.server[0], "done");
    let state = markReducer(start, { type: "optimistic", item: optimistic });
    const reply = { ...optimistic, comments: 7 };
    state = markReducer(state, { type: "settled", item: reply });
    expect(state.overrides).toEqual({});
    expect(withOverrides(state)[0].comments).toBe(7);
  });

  it("rolls back when the desktop refuses", () => {
    let state = markReducer(start, { type: "optimistic", item: applyMark(start.server[0], "done") });
    state = markReducer(state, { type: "rolledBack", key: "a" });
    expect(withOverrides(state)[0]).toBe(start.server[0]);
  });
});

describe("cache and staleness", () => {
  it("reads a saved page and rejects junk", () => {
    const page = { items: [item({ key: "a" })], fetchedAt: 5, providers: [], counts: { unread: 1, needsMe: 1, done: 0 } };
    expect(parseCachedPage(JSON.stringify(page))?.items).toHaveLength(1);
    expect(parseCachedPage("not json")).toBeNull();
    expect(parseCachedPage(JSON.stringify({ items: "x" }))).toBeNull();
    expect(parseCachedPage(null)).toBeNull();
    expect(parseCachedPage("")).toBeNull();
  });

  const loaded = { loaded: true, offline: false, stale: false, fetchedAt: NOW, providers: [] as { id: string; connected: boolean; error?: string }[] };

  it("offline wins over stale and says when the list was last read", () => {
    expect(bannerFor({ ...loaded, offline: true, stale: true })).toEqual({ kind: "offline", fetchedAt: NOW });
  });

  it("calls a list stale when the desktop says so or a provider fails", () => {
    expect(bannerFor({ ...loaded, stale: true })).toEqual({ kind: "stale", fetchedAt: NOW, failing: [] });
    expect(
      bannerFor({ ...loaded, providers: [{ id: "jira", connected: false, error: "expired" }, { id: "github", connected: true }] }),
    ).toEqual({ kind: "stale", fetchedAt: NOW, failing: ["jira"] });
  });

  it("has no banner for a fresh list or one still loading", () => {
    expect(bannerFor(loaded)).toBeNull();
    expect(bannerFor({ ...loaded, loaded: false, fetchedAt: null })).toBeNull();
    expect(bannerFor({ ...loaded, loaded: false, fetchedAt: null, offline: true })).toEqual({ kind: "offline", fetchedAt: null });
  });

  it("keys lists by what they ask for", () => {
    expect(feedKey({ scope: "all" })).not.toBe(feedKey({ scope: "all", projectId: "p" }));
    expect(feedKey({ scope: "done", projectId: "p" })).toBe("done|p|*|");
  });
});

describe("item sheet helpers", () => {
  it("words a check's outcome and time", () => {
    expect(checkLabel({ name: "a", state: "failing", durationMs: 400_000 })).toBe("failed · 6m 40s");
    expect(checkLabel({ name: "a", state: "passing", durationMs: 41_000 })).toBe("41s");
    expect(checkLabel({ name: "a", state: "pending" })).toBe("running");
    expect(checkLabel({ name: "a", state: "skipped" })).toBe("skipped");
    expect(checkLabel({ name: "a", state: "passing", durationMs: 120_000 })).toBe("2m");
  });

  it("hands a diff to the diff screen with null for the numbers it lacks", () => {
    const file = toChangedFile({
      path: "src/a.ts",
      additions: 1,
      deletions: 1,
      lines: [
        { kind: "hunk", text: "@@ -1 +1 @@" },
        { kind: "del", text: "old", oldNo: 1 },
        { kind: "add", text: "new", newNo: 1 },
      ],
    });
    expect(file).toMatchObject({ relative: "src/a.ts", status: "modified", additions: 1, deletions: 1 });
    expect(file.lines.map((line) => [line.kind, line.oldNumber, line.newNumber])).toEqual([
      ["hunk", null, null],
      ["del", 1, null],
      ["add", null, 1],
    ]);
  });
});
