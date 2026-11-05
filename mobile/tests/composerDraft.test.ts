import { expect, it } from "vitest";

import {
  cachedText,
  freshSync,
  HOLD_MS,
  localMark,
  MARK_HOLD_MS,
  received,
  sent,
  settle,
  startWrite,
  syncFor,
  typed,
  unsent,
  writeFailed,
  written,
} from "@/features/sessions/composerDraft";

it("shows the desktop's text when the thread opens", () => {
  const sync = freshSync();
  expect(startWrite(sync)).toBeNull();
  expect(received(sync, { text: "from the desktop", rev: 5 }, 0)).toBe("from the desktop");
  expect(unsent(sync)).toBe(false);
});

it("keeps what the person typed before the first read came back", () => {
  const sync = freshSync();
  typed(sync, "quick", 10);
  expect(received(sync, { text: "older", rev: 5 }, 20)).toBeNull();
  expect(startWrite(sync)).toEqual({ text: "quick", base: 5, epoch: 0 });
});

it("writes the phone's text against the revision it was based on", () => {
  const sync = freshSync();
  received(sync, { text: "", rev: 0 }, 0);
  typed(sync, "hello", 100);
  const write = startWrite(sync)!;
  expect(write).toEqual({ text: "hello", base: 0, epoch: 0 });
  // One at a time.
  expect(startWrite(sync)).toBeNull();
  typed(sync, "hello there", 200);
  expect(written(sync, write, { accepted: true, text: "hello", rev: 9 }, 300)).toBeNull();
  // Typed on during the write: it goes next, on the new revision.
  expect(startWrite(sync)).toEqual({ text: "hello there", base: 9, epoch: 0 });
});

it("does not replace text the person is typing, and takes it once they stop", () => {
  const sync = freshSync();
  received(sync, { text: "a", rev: 1 }, 0);
  typed(sync, "a", 1000);
  expect(received(sync, { text: "from the desktop", rev: 2 }, 1500)).toBeNull();
  expect(sync.text).toBe("a");
  expect(settle(sync, 1000 + HOLD_MS - 1)).toBeNull();
  expect(settle(sync, 1000 + HOLD_MS)).toBe("from the desktop");
  expect(sync.base).toBe(2);
});

it("keeps the person's text when they are typing and the desktop changed too", () => {
  const sync = freshSync();
  received(sync, { text: "a", rev: 1 }, 0);
  typed(sync, "ab", 1000);
  const write = startWrite(sync)!;
  expect(
    written(sync, write, { accepted: false, text: "desktop", rev: 4 }, 1700),
  ).toBeNull();
  expect(sync.text).toBe("ab");
  expect(startWrite(sync)).toEqual({ text: "ab", base: 4, epoch: 0 });
});

it("lets the desktop's newer text win over a write that waited offline", () => {
  const sync = freshSync();
  received(sync, { text: "a", rev: 1 }, 0);
  typed(sync, "typed offline", 1000);
  // The link was down: the write fails and waits.
  const failed = startWrite(sync)!;
  writeFailed(sync);
  expect(failed.base).toBe(1);
  // Back much later, the desktop has moved on.
  const late = startWrite(sync)!;
  expect(
    written(sync, late, { accepted: false, text: "newer on the desktop", rev: 8 }, 60_000),
  ).toBe("newer on the desktop");
  expect(unsent(sync)).toBe(false);
  expect(startWrite(sync)).toBeNull();
});

it("sending clears the field and ignores answers to writes from before", () => {
  const sync = freshSync();
  received(sync, { text: "", rev: 0 }, 0);
  typed(sync, "hel", 100);
  const write = startWrite(sync)!;
  typed(sync, "hello", 200);
  expect(sent(sync)).toBe(0);
  expect(sync.text).toBe("");
  expect(written(sync, write, { accepted: false, text: "", rev: 0 }, 300)).toBeNull();
  expect(startWrite(sync)).toBeNull();
  // The desktop's cleared copy changes nothing.
  expect(received(sync, { text: "", rev: 0 }, 400)).toBeNull();
  expect(sync.text).toBe("");
});

it("blank text the desktop drops is still the phone's to keep", () => {
  const sync = freshSync();
  received(sync, { text: "", rev: 0 }, 0);
  typed(sync, "  ", 100);
  const write = startWrite(sync)!;
  written(sync, write, { accepted: true, text: "", rev: 0 }, 200);
  expect(unsent(sync)).toBe(false);
  expect(startWrite(sync)).toBeNull();
});

it("remembers each thread's text while the app runs", () => {
  const sync = syncFor("thread-1");
  received(sync, { text: "kept", rev: 3 }, 0);
  expect(cachedText("thread-1")).toBe("kept");
  expect(syncFor("thread-1")).toBe(sync);
  expect(cachedText("thread-2")).toBe("");
  expect(cachedText()).toBe("");
});

const NOW = 1_000_000;

it("leaves a thread's mark to the desktop until something changes here", () => {
  expect(localMark(freshSync(), NOW)).toBeNull();
});

it("marks text typed here at once, however long it waits for the desktop", () => {
  const sync = freshSync();
  typed(sync, "offline thought", NOW);
  expect(localMark(sync, NOW)).toBe(true);
  expect(localMark(sync, NOW + MARK_HOLD_MS * 100)).toBe(true);
});

it("doesn't mark blank text", () => {
  const sync = freshSync();
  typed(sync, "   ", NOW);
  expect(localMark(sync, NOW)).toBe(false);
});

it("keeps the mark while the desktop's word catches up with a write, then lets go", () => {
  const sync = freshSync();
  received(sync, { text: "", rev: 1 }, NOW);
  typed(sync, "saved soon", NOW);
  const write = startWrite(sync)!;
  written(sync, write, { text: "saved soon", rev: 2, accepted: true }, NOW + 100);
  expect(localMark(sync, NOW + 100 + MARK_HOLD_MS - 1)).toBe(true);
  expect(localMark(sync, NOW + 100 + MARK_HOLD_MS)).toBeNull();
});

it("drops the mark as soon as the text is cleared or sent", () => {
  const cleared = freshSync();
  received(cleared, { text: "old", rev: 1 }, NOW);
  typed(cleared, "", NOW);
  expect(localMark(cleared, NOW)).toBe(false);

  const sending = freshSync();
  received(sending, { text: "", rev: 1 }, NOW);
  typed(sending, "go", NOW);
  sent(sending, NOW + 1);
  expect(localMark(sending, NOW + 1)).toBe(false);
});
