import { describe, expect, it } from "vitest";

import {
  follow,
  freshSync,
  newer,
  parseMessage,
  PUSH_GRACE_MS,
  reportState,
  sendScript,
  TOUCH_WINDOW_MS,
  trusted,
  type LiveDraft,
} from "@/features/drafts/sync";

const record = (over: Partial<LiveDraft> = {}): LiveDraft => ({
  live: true,
  title: "Pricing",
  rev: 5,
  generation: 1,
  kind: "html",
  state: { radius: 8, glow: false },
  stateRev: 3,
  pins: [],
  pending: [],
  control: false,
  waiting: false,
  ...over,
});

describe("reportState", () => {
  it("keeps what was chosen elsewhere over a page's starting values", () => {
    const sync = freshSync();
    const result = reportState(sync, record(), { radius: 4, glow: false }, -Infinity, 1000);
    expect(result.page).toEqual([{ __mcState: { radius: 8, glow: false } }]);
    expect(result.desktop).toBeNull();
    expect(sync.stateRev).toBe(3);
    // The page's report before it caught up is not the person's change.
    const early = reportState(sync, record(), { radius: 4, glow: false }, 1100, 1200);
    expect(early).toEqual({ page: [], desktop: null });
  });

  it("starts the record with the person's change when no page has reported", () => {
    const sync = freshSync();
    const empty = record({ state: null });
    expect(reportState(sync, empty, { radius: 8 }, -Infinity, 0)).toEqual({
      page: [],
      desktop: null,
    });
    const now = 10_000;
    expect(reportState(sync, empty, { radius: 9 }, -Infinity, now).desktop).toBeNull();
    expect(reportState(sync, empty, { radius: 12 }, now - 100, now).desktop).toEqual({
      action: "state",
      values: { radius: 12 },
    });
  });

  it("sends the keys the person changed, right after their touch", () => {
    const sync = freshSync();
    reportState(sync, record(), { radius: 8, glow: false }, -Infinity, 0);
    const now = 10_000;
    const result = reportState(sync, record(), { radius: 12, glow: false }, now - 100, now);
    expect(result.desktop).toEqual({ action: "state", values: { radius: 12 } });
  });

  it("leaves a change the page made by itself on this screen", () => {
    const sync = freshSync();
    reportState(sync, record(), { radius: 8, glow: false }, -Infinity, 0);
    const now = 10_000;
    const result = reportState(
      sync,
      record(),
      { radius: 12, glow: false },
      now - TOUCH_WINDOW_MS - 1,
      now,
    );
    expect(result.desktop).toBeNull();
  });

  it("stops waiting for a page that never takes the values it was given", () => {
    const sync = freshSync();
    reportState(sync, record(), { radius: 4, glow: false }, -Infinity, 0);
    const later = PUSH_GRACE_MS + 1;
    const result = reportState(sync, record(), { radius: 4, glow: true }, later - 10, later);
    expect(sync.pushed).toBeNull();
    expect(result.desktop).toEqual({ action: "state", values: { radius: 4, glow: true } });
  });
});

describe("follow", () => {
  it("gives the page another screen's state and pins, but not its own pins", () => {
    const sync = freshSync();
    reportState(sync, record(), { radius: 8, glow: false }, -Infinity, 0);
    expect(
      follow(
        sync,
        record({
          stateRev: 4,
          state: { radius: 2, glow: false },
          pins: [
            { x: 10, y: 20, side: "desktop" },
            { x: 30, y: 40, side: "phone" },
          ],
        }),
        0,
      ),
    ).toEqual([
      // A page that just loaded has no pins at all.
      { __mcPin: { x: 10, y: 20 } },
      { __mcPin: { x: 30, y: 40 } },
      { __mcState: { radius: 2, glow: false } },
    ]);
    const more = record({
      stateRev: 4,
      state: { radius: 2, glow: false },
      pins: [
        { x: 10, y: 20, side: "desktop" },
        { x: 30, y: 40, side: "phone" },
        { x: 50, y: 60, side: "phone" },
        { x: 70, y: 80, side: "desktop" },
      ],
    });
    expect(follow(sync, more, 0)).toEqual([{ __mcPin: { x: 70, y: 80 } }]);
    const at = { path: "0.1", ax: 0.5, ay: 0.5 };
    expect(
      follow(sync, { ...more, pins: [...more.pins, { x: 5, y: 5, side: "desktop", at }] }, 0),
    ).toEqual([{ __mcPin: { x: 5, y: 5, at } }]);
  });

  it("holds the page while the person's own change is on its way", () => {
    const sync = freshSync();
    reportState(sync, record(), { radius: 8, glow: false }, -Infinity, 0);
    sync.sending = 1;
    sync.reported = { radius: 14, glow: false };
    const answered = record({ stateRev: 4, state: { radius: 12, glow: false } });
    expect(follow(sync, answered, 0)).toEqual([]);
    sync.sending = 0;
    expect(follow(sync, answered, 0)).toEqual([{ __mcState: { radius: 12, glow: false } }]);
  });
});

describe("trusted", () => {
  const now = 100_000;
  it("takes taps and page messages only right after the person's touch", () => {
    expect(trusted({ k: "u" }, now - 100, false, now)).toBe(true);
    expect(trusted({ k: "n", t: "Use this" }, now - TOUCH_WINDOW_MS - 1, false, now)).toBe(false);
  });
  it("takes a note only in comment mode", () => {
    const note = { k: "f" as const, f: { note: "Bigger", tag: "button", name: "Go", x: 1, y: 2 } };
    expect(trusted(note, now - 30_000, true, now)).toBe(true);
    expect(trusted(note, now - 100, false, now)).toBe(false);
  });
});

describe("parseMessage", () => {
  it("reads the bridge's messages and nothing else", () => {
    expect(parseMessage(JSON.stringify({ __mc: 1, k: "v", v: { a: 1 } }))).toEqual({
      k: "v",
      v: { a: 1 },
    });
    expect(parseMessage(JSON.stringify({ __mc: 1, k: "v", v: [1] }))).toBeNull();
    expect(parseMessage(JSON.stringify({ __mc: 1, k: "h", v: 320 }))).toEqual({ k: "h", v: 320 });
    expect(parseMessage(JSON.stringify({ __mc: 1, k: "h", v: "tall" }))).toBeNull();
    expect(parseMessage(JSON.stringify({ k: "u" }))).toBeNull();
    expect(parseMessage(JSON.stringify({ __mc: 1, k: "f", f: { note: "x" } }))).toBeNull();
    expect(parseMessage("not json")).toBeNull();
    // The element a note sits on comes along, as long as it is one a page could name.
    const at = { path: "0.2.1", ax: 0.25, ay: 0.5 };
    const note = { note: "Bigger", tag: "b", name: "Pro", x: 10, y: 20 };
    expect(parseMessage(JSON.stringify({ __mc: 1, k: "f", f: { ...note, at } }))).toEqual({
      k: "f",
      f: { ...note, at },
    });
    expect(
      parseMessage(JSON.stringify({ __mc: 1, k: "f", f: { ...note, at: { ...at, path: "a" } } })),
    ).toEqual({ k: "f", f: note });
  });
});

describe("newer", () => {
  it("never lets an older answer replace a newer page or revision", () => {
    const current = record({ rev: 9, generation: 2, page: "<p>2</p>" });
    expect(newer(current, record({ rev: 12, generation: 1 }))).toBe(current);
    expect(newer(current, record({ rev: 8, generation: 2 }))).toBe(current);
    const next = newer(current, record({ rev: 10, generation: 2 }));
    expect(next.live && next.page).toBe("<p>2</p>");
    expect(newer(current, { live: false })).toEqual({ live: false });
  });
});

it("hands a command to the page as data", () => {
  expect(sendScript({ __mcComment: true })).toBe(
    'window.__mcSend&&window.__mcSend({"__mcComment":true});true;',
  );
});
