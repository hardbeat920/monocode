import { afterEach, beforeEach, describe, expect, it } from "vitest";
import {
  inboxHasUnseenItems,
  inboxSeenIsSeeded,
  isInboxEntryUnseen,
  markInboxItemSeen,
  markInboxItemsSeen,
  seedInboxSeenIfNeeded,
} from "./inboxSeen";

const KEY = "monocode.inboxSeen";

function mockLocalStorage() {
  const data = new Map<string, string>();
  const storage = {
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => {
      data.set(key, value);
    },
    removeItem: (key: string) => {
      data.delete(key);
    },
    clear: () => {
      data.clear();
    },
    key: (index: number) => [...data.keys()][index] ?? null,
    get length() {
      return data.size;
    },
  };
  Object.defineProperty(globalThis, "localStorage", {
    value: storage,
    configurable: true,
  });
}

function entry(
  key: string,
  updatedAt: string,
): { key: string; updatedAt: string } {
  return { key, updatedAt };
}

describe("inbox seen items", () => {
  beforeEach(mockLocalStorage);
  afterEach(() => {
    localStorage.removeItem(KEY);
  });

  it("seeds current items so a long-standing inbox does not badge", () => {
    seedInboxSeenIfNeeded([
      entry("linear:ENG-1", "2026-08-27T10:00:00Z"),
      entry("github:acme/web:issue:4", "2026-08-27T11:00:00Z"),
    ]);
    expect(
      inboxHasUnseenItems([
        entry("linear:ENG-1", "2026-08-27T10:00:00Z"),
        entry("github:acme/web:issue:4", "2026-08-27T11:00:00Z"),
      ]),
    ).toBe(false);
  });

  it("flags a newly appeared item", () => {
    seedInboxSeenIfNeeded([entry("linear:ENG-1", "2026-08-27T10:00:00Z")]);
    expect(
      inboxHasUnseenItems([
        entry("linear:ENG-1", "2026-08-27T10:00:00Z"),
        entry("linear:ENG-2", "2026-08-27T10:01:00Z"),
      ]),
    ).toBe(true);
  });

  it("flags an existing item that got a newer update", () => {
    seedInboxSeenIfNeeded([entry("linear:ENG-1", "2026-08-27T10:00:00Z")]);
    expect(
      inboxHasUnseenItems([entry("linear:ENG-1", "2026-08-27T11:00:00Z")]),
    ).toBe(true);
  });

  it("does not flag the same items again", () => {
    const items = [entry("linear:ENG-1", "2026-08-27T10:00:00Z")];
    seedInboxSeenIfNeeded(items);
    expect(inboxHasUnseenItems(items)).toBe(false);
  });

  it("clears one item when that card is opened, leaving other unseen", () => {
    seedInboxSeenIfNeeded([entry("linear:ENG-1", "2026-08-27T10:00:00Z")]);
    const next = [
      entry("linear:ENG-1", "2026-08-27T10:00:00Z"),
      entry("linear:ENG-2", "2026-08-27T10:01:00Z"),
    ];
    expect(inboxHasUnseenItems(next)).toBe(true);
    markInboxItemSeen(next[1]!);
    expect(isInboxEntryUnseen(next[1]!)).toBe(false);
    expect(isInboxEntryUnseen(next[0]!)).toBe(false);
    expect(inboxHasUnseenItems(next)).toBe(false);
  });

  it("keeps the badge until every new card has been opened", () => {
    seedInboxSeenIfNeeded([entry("linear:ENG-1", "2026-08-27T10:00:00Z")]);
    const next = [
      entry("linear:ENG-1", "2026-08-27T10:00:00Z"),
      entry("linear:ENG-2", "2026-08-27T10:01:00Z"),
      entry("linear:ENG-3", "2026-08-27T10:02:00Z"),
    ];
    expect(inboxHasUnseenItems(next)).toBe(true);
    markInboxItemSeen(next[1]!);
    expect(inboxHasUnseenItems(next)).toBe(true);
    markInboxItemSeen(next[2]!);
    expect(inboxHasUnseenItems(next)).toBe(false);
  });

  it("marks all supplied items as seen", () => {
    seedInboxSeenIfNeeded([entry("linear:ENG-1", "2026-08-27T10:00:00Z")]);
    const next = [
      entry("linear:ENG-2", "2026-08-27T10:01:00Z"),
      entry("linear:ENG-3", "2026-08-27T10:02:00Z"),
    ];
    expect(inboxHasUnseenItems(next)).toBe(true);
    markInboxItemsSeen(next);
    expect(inboxHasUnseenItems(next)).toBe(false);
  });

  it("treats a new item with an unreadable timestamp as unseen", () => {
    seedInboxSeenIfNeeded([entry("linear:ENG-1", "2026-08-27T10:00:00Z")]);
    expect(inboxHasUnseenItems([entry("linear:ENG-2", "not-a-date")])).toBe(
      true,
    );
  });

  it("is seeded after the first snapshot", () => {
    expect(inboxSeenIsSeeded()).toBe(false);
    seedInboxSeenIfNeeded([entry("linear:ENG-1", "2026-08-27T10:00:00Z")]);
    expect(inboxSeenIsSeeded()).toBe(true);
  });

  it("does not treat existing items as new if a card is opened before seed", () => {
    markInboxItemSeen(entry("linear:ENG-2", "2026-08-27T10:01:00Z"));
    expect(inboxSeenIsSeeded()).toBe(false);
    seedInboxSeenIfNeeded([
      entry("linear:ENG-1", "2026-08-27T10:00:00Z"),
      entry("linear:ENG-2", "2026-08-27T10:01:00Z"),
    ]);
    expect(
      inboxHasUnseenItems([
        entry("linear:ENG-1", "2026-08-27T10:00:00Z"),
        entry("linear:ENG-2", "2026-08-27T10:01:00Z"),
      ]),
    ).toBe(false);
  });
});
