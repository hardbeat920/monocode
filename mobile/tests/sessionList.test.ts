import { describe, expect, it, vi } from "vitest";

vi.mock("@/features/workspace/WorkspaceProvider", () => ({ useWorkspace: vi.fn() }));

import { ageLabel, dayHeadings } from "@/features/sessions/dayHeadings";
import { refreshedPages, visibleSessions } from "@/features/sessions/useSessionPages";
import type { SessionSummary } from "@/features/workspace/types";

const session = (id: string, updatedAt: number, title = id): SessionSummary => ({
  id,
  projectId: "p",
  title,
  harness: "claude",
  model: "opus",
  branch: "main",
  updatedAt,
});
const ids = (sessions: SessionSummary[]) => sessions.map((s) => s.id);
const pages = (sessions: SessionSummary[]) =>
  ({ sessions, hasMore: false, loadMore: () => {}, loadingMore: false }) as never;
const keep = (s: SessionSummary) => s;
const polledFrom = (sessions: SessionSummary[]) => new Set(ids(sessions));

describe("visibleSessions", () => {
  it("moves a row when the poll says it was active more recently than the page read", () => {
    const read = [session("a", 300), session("b", 200), session("c", 100)];
    const recent = [session("c", 400), session("a", 300), session("b", 200)];

    expect(ids(visibleSessions(pages(read), recent, "", keep, polledFrom(recent)))).toEqual([
      "c",
      "a",
      "b",
    ]);
  });

  it("lists a session started since the page was read", () => {
    const read = [session("a", 300)];
    const recent = [session("new", 400), session("a", 300)];

    expect(ids(visibleSessions(pages(read), recent, "", keep, polledFrom(recent)))).toEqual([
      "new",
      "a",
    ]);
  });

  it("keeps a page's row the poll no longer carries in its place", () => {
    const read = [session("a", 300), session("old", 100)];
    const recent = [session("a", 300)];

    expect(ids(visibleSessions(pages(read), recent, "", keep, polledFrom(recent)))).toEqual([
      "a",
      "old",
    ]);
  });

  it("leaves out a session only remembered here, which may be archived", () => {
    const read = [session("a", 300)];
    const remembered = session("archived", 400);

    expect(
      ids(visibleSessions(pages(read), [remembered, ...read], "", keep, polledFrom(read))),
    ).toEqual(["a"]);
  });

  it("adds only the poll's sessions that match the search", () => {
    const read = [session("a", 300, "fix the list")];
    const recent = [session("b", 500, "other"), session("c", 400, "the list again"), ...read];

    expect(ids(visibleSessions(pages(read), recent, "list", keep, polledFrom(recent)))).toEqual([
      "c",
      "a",
    ]);
  });

  it("hides a session deleted here", () => {
    const read = [session("a", 300), session("gone", 200)];
    const removed = (s: SessionSummary) => (s.id === "gone" ? null : s);

    expect(ids(visibleSessions(pages(read), [], "", removed, new Set()))).toEqual(["a"]);
  });
});

describe("refreshedPages", () => {
  const scrolled = {
    sessions: [session("a", 500), session("b", 400), session("c", 300), session("d", 200)],
    hasMore: true,
  };

  it("keeps the rows past a first page read again", () => {
    const page = { sessions: [session("new", 600), session("a", 500)], hasMore: true };

    expect(ids(refreshedPages(scrolled, page).sessions)).toEqual(["new", "a", "b", "c", "d"]);
    expect(refreshedPages(scrolled, page).hasMore).toBe(true);
  });

  it("drops a newer row missing from the first page, which left the list", () => {
    const page = { sessions: [session("a", 500), session("c", 300)], hasMore: true };

    expect(ids(refreshedPages(scrolled, page).sessions)).toEqual(["a", "c", "d"]);
  });

  it("takes a last page as it is", () => {
    const page = { sessions: [session("a", 500)], hasMore: false };

    expect(refreshedPages(scrolled, page)).toBe(page);
  });

  it("says no more remain when the rows kept were the end of the list", () => {
    const page = { sessions: [session("a", 500)], hasMore: true };

    expect(refreshedPages({ ...scrolled, hasMore: false }, page).hasMore).toBe(false);
  });
});

describe("day headings", () => {
  const now = new Date(2026, 10, 5, 0, 30).getTime();
  const at = (date: number, hour = 12) => new Date(2026, 10, date, hour).getTime();

  it("goes by calendar days, as the desktop does", () => {
    expect(ageLabel(new Date(2026, 10, 5, 0, 10).getTime(), now)).toBe("Today");
    expect(ageLabel(at(4, 23), now)).toBe("Yesterday");
    expect(ageLabel(at(3), now)).toBe("Last 7 days");
    expect(ageLabel(new Date(2026, 9, 30).getTime(), now)).toBe("Last 7 days");
    expect(ageLabel(new Date(2026, 9, 29).getTime(), now)).toBe("Older");
    expect(ageLabel(0, now)).toBe("Older");
  });

  it("treats a clock running ahead of the phone's as today", () => {
    expect(ageLabel(at(6), now)).toBe("Today");
  });

  it("marks the first row of each run", () => {
    const list = [
      session("a", now),
      session("b", now - 1),
      session("c", at(4)),
      session("d", at(1)),
      session("e", at(1) - 1),
    ];

    expect([...dayHeadings(list, now)]).toEqual([
      ["a", "Today"],
      ["c", "Yesterday"],
      ["d", "Last 7 days"],
    ]);
  });
});
