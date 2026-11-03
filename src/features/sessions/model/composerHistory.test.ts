import { afterEach, beforeEach, describe, expect, it } from "vitest";
import {
  clearComposerHistory,
  composerHistoryDirection,
  COMPOSER_HISTORY_LIMIT,
  EMPTY_COMPOSER_HISTORY_CURSOR,
  loadComposerHistory,
  parseComposerHistory,
  pushComposerHistory,
  recordComposerHistory,
  stepComposerHistory,
} from "./composerHistory";

function key(
  partial: Partial<Parameters<typeof composerHistoryDirection>[0]> & {
    key: string;
  },
) {
  return {
    altKey: false,
    ctrlKey: false,
    metaKey: false,
    shiftKey: false,
    selectionStart: 0,
    selectionEnd: 0,
    value: "",
    ...partial,
  };
}

describe("pushComposerHistory", () => {
  it("drops blank entries and consecutive duplicates", () => {
    expect(pushComposerHistory([], "  \n")).toEqual([]);
    expect(pushComposerHistory(["hello"], "hello")).toEqual(["hello"]);
    expect(pushComposerHistory(["hello"], "Hello")).toEqual(["hello", "Hello"]);
  });

  it("normalizes CRLF and keeps the newest entries within the cap", () => {
    expect(pushComposerHistory([], "a\r\nb")).toEqual(["a\nb"]);
    const filled = Array.from(
      { length: COMPOSER_HISTORY_LIMIT },
      (_, i) => `p${i}`,
    );
    expect(pushComposerHistory(filled, "newest")).toEqual([
      ...filled.slice(1),
      "newest",
    ]);
  });
});

describe("composerHistoryDirection", () => {
  it("recalls on the first line and the last line only", () => {
    expect(composerHistoryDirection(key({ key: "ArrowUp" }))).toBe("up");
    expect(
      composerHistoryDirection(
        key({
          key: "ArrowUp",
          value: "ab\ncd",
          selectionStart: 1,
          selectionEnd: 1,
        }),
      ),
    ).toBe("up");
    expect(
      composerHistoryDirection(
        key({
          key: "ArrowUp",
          value: "ab\ncd",
          selectionStart: 3,
          selectionEnd: 3,
        }),
      ),
    ).toBeNull();
    expect(
      composerHistoryDirection(
        key({
          key: "ArrowDown",
          value: "ab\ncd",
          selectionStart: 4,
          selectionEnd: 4,
        }),
      ),
    ).toBe("down");
    expect(
      composerHistoryDirection(
        key({
          key: "ArrowDown",
          value: "ab\ncd",
          selectionStart: 1,
          selectionEnd: 1,
        }),
      ),
    ).toBeNull();
  });

  it("ignores modified keys and a non-collapsed selection", () => {
    expect(
      composerHistoryDirection(key({ key: "ArrowUp", metaKey: true })),
    ).toBeNull();
    expect(
      composerHistoryDirection(key({ key: "ArrowUp", shiftKey: true })),
    ).toBeNull();
    expect(
      composerHistoryDirection(
        key({
          key: "ArrowUp",
          selectionStart: 0,
          selectionEnd: 2,
          value: "ab",
        }),
      ),
    ).toBeNull();
  });
});

describe("stepComposerHistory", () => {
  const entries = ["first", "second", "third"];

  it("walks from the live draft back through older prompts", () => {
    const start = stepComposerHistory(
      entries,
      EMPTY_COMPOSER_HISTORY_CURSOR,
      "up",
      "draft",
    );
    expect(start).toEqual({
      cursor: { index: 2, stash: "draft" },
      text: "third",
    });

    const older = stepComposerHistory(entries, start.cursor, "up", start.text);
    expect(older.text).toBe("second");

    const oldest = stepComposerHistory(entries, older.cursor, "up", older.text);
    expect(oldest).toEqual({
      cursor: { index: 0, stash: "draft" },
      text: "first",
    });

    expect(
      stepComposerHistory(entries, oldest.cursor, "up", oldest.text),
    ).toEqual(oldest);
  });

  it("restores the stashed draft after the newest entry", () => {
    const recalled = stepComposerHistory(
      entries,
      EMPTY_COMPOSER_HISTORY_CURSOR,
      "up",
      "half typed",
    );
    const restored = stepComposerHistory(
      entries,
      recalled.cursor,
      "down",
      recalled.text,
    );
    expect(restored).toEqual({
      cursor: EMPTY_COMPOSER_HISTORY_CURSOR,
      text: "half typed",
    });
    expect(
      stepComposerHistory(entries, restored.cursor, "down", restored.text),
    ).toEqual(restored);
  });

  it("does nothing without stored prompts", () => {
    expect(
      stepComposerHistory([], EMPTY_COMPOSER_HISTORY_CURSOR, "up", "draft"),
    ).toEqual({
      cursor: EMPTY_COMPOSER_HISTORY_CURSOR,
      text: "draft",
    });
  });
});

describe("recordComposerHistory", () => {
  beforeEach(() => {
    clearComposerHistory();
  });

  afterEach(() => {
    clearComposerHistory();
  });

  it("persists across loads and skips a blank record", () => {
    recordComposerHistory("one");
    recordComposerHistory("   ");
    recordComposerHistory("one");
    recordComposerHistory("two");
    expect(loadComposerHistory()).toEqual(["one", "two"]);
  });
});

describe("parseComposerHistory", () => {
  it("keeps non-empty strings and ignores anything else", () => {
    expect(parseComposerHistory("{")).toEqual([]);
    expect(parseComposerHistory("null")).toEqual([]);
    expect(parseComposerHistory('["ok", 1, "", "also"]')).toEqual([
      "ok",
      "also",
    ]);
  });
});
