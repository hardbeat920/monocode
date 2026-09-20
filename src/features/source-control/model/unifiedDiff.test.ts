import { describe, expect, it } from "vitest";
import {
  blocksFromLines,
  buildUnifiedFile,
  expandFold,
  foldUnifiedLines,
  revealedFold,
  type UnifiedLine,
} from "./unifiedDiff";

describe("buildUnifiedFile", () => {
  it("marks a replacement as a delete then an add", () => {
    const diff = buildUnifiedFile("alpha\nbeta\ngamma\n", "alpha\nBETA\ngamma\n");
    expect(diff.additions).toBe(1);
    expect(diff.deletions).toBe(1);
    const changed = diff.lines.filter((line) => line.kind !== "context");
    expect(changed).toEqual([
      expect.objectContaining({
        kind: "del",
        text: "beta",
        oldNumber: 2,
        newNumber: null,
      }),
      expect.objectContaining({
        kind: "add",
        text: "BETA",
        oldNumber: null,
        newNumber: 2,
      }),
    ]);
  });

  it("treats a new file as additions", () => {
    const diff = buildUnifiedFile("", "one\ntwo\n");
    expect(diff.deletions).toBe(0);
    expect(diff.additions).toBeGreaterThan(0);
    expect(diff.lines.every((line) => line.kind === "add")).toBe(true);
  });

  it("treats a deleted file as deletions", () => {
    const diff = buildUnifiedFile("one\ntwo\n", "");
    expect(diff.additions).toBe(0);
    expect(diff.deletions).toBeGreaterThan(0);
    expect(diff.lines.every((line) => line.kind === "del")).toBe(true);
  });

  it("folds unmodified runs outside the context window", () => {
    const original = Array.from({ length: 20 }, (_, i) => `line-${i + 1}`).join(
      "\n",
    );
    const current = original.replace("line-10", "LINE-10");
    const diff = buildUnifiedFile(original, current, 3);
    const folds = diff.blocks.filter((block) => block.kind === "fold");
    expect(folds).toHaveLength(2);
    expect(folds[0]).toMatchObject({ kind: "fold" });
    expect(folds[1]).toMatchObject({ kind: "fold" });
    if (folds[0].kind !== "fold" || folds[1].kind !== "fold") return;
    expect(folds[0].lines[0]?.text).toBe("line-1");
    expect(folds[0].lines.at(-1)?.text).toBe("line-6");
    expect(folds[1].lines[0]?.text).toBe("line-14");
  });

  it("keeps a tiny file fully visible when every line is near a change", () => {
    const diff = buildUnifiedFile("a\nb\nc\n", "a\nB\nc\n", 3);
    expect(diff.blocks.every((block) => block.kind === "hunk")).toBe(true);
  });

  it("returns no add/del for identical files", () => {
    const diff = buildUnifiedFile("same\nfile\n", "same\nfile\n");
    expect(diff.additions).toBe(0);
    expect(diff.deletions).toBe(0);
    expect(diff.lines.every((line) => line.kind === "context")).toBe(true);
  });
});

describe("foldUnifiedLines", () => {
  it("splits long context around a change into folds", () => {
    const lines: UnifiedLine[] = [
      ...rangeContext(1, 8),
      { kind: "del", text: "old", oldNumber: 9, newNumber: null },
      { kind: "add", text: "new", oldNumber: null, newNumber: 9 },
      ...rangeContext(10, 16),
    ];
    const blocks = foldUnifiedLines(lines, 2);
    expect(blocks.map((block) => block.kind)).toEqual(["fold", "hunk", "fold"]);
    const hunk = blocks[1];
    expect(hunk.kind).toBe("hunk");
    if (hunk.kind !== "hunk") return;
    expect(hunk.lines.map((line) => line.text)).toEqual([
      "c7",
      "c8",
      "old",
      "new",
      "c10",
      "c11",
    ]);
  });

  it("keeps every line visible when context is unlimited", () => {
    const lines: UnifiedLine[] = [
      ...rangeContext(1, 8),
      { kind: "del", text: "old", oldNumber: 9, newNumber: null },
      { kind: "add", text: "new", oldNumber: null, newNumber: 9 },
      ...rangeContext(10, 16),
    ];
    const blocks = foldUnifiedLines(lines, Number.POSITIVE_INFINITY);
    expect(blocks.map((block) => block.kind)).toEqual(["hunk"]);
    expect(blocks[0]?.kind === "hunk" && blocks[0].lines).toEqual(lines);
  });
});

describe("blocksFromLines", () => {
  it("keeps hunk headers visible so they split folds", () => {
    const blocks = blocksFromLines(
      [
        { kind: "hunk", text: "@@ -1,3 +1,3 @@", oldNumber: null, newNumber: null },
        { kind: "context", text: "keep", oldNumber: 1, newNumber: 1 },
        { kind: "add", text: "plus", oldNumber: null, newNumber: 2 },
      ],
      1,
    );
    expect(blocks[0]?.kind).toBe("hunk");
  });
});

describe("fold expansion", () => {
  it("reveals from the start when expanding down", () => {
    expect(revealedFold(10, expandFold(undefined, 10, "down", 3))).toEqual({
      head: 3,
      tail: 0,
      hidden: 7,
    });
  });

  it("reveals from the end when expanding up", () => {
    const next = expandFold({ start: 2, end: 0 }, 10, "up", 3);
    expect(revealedFold(10, next)).toEqual({
      head: 2,
      tail: 3,
      hidden: 5,
    });
  });

  it("expands the whole fold at once", () => {
    expect(revealedFold(12, expandFold(undefined, 12, "all"))).toEqual({
      head: 12,
      tail: 0,
      hidden: 0,
    });
  });
});

function rangeContext(from: number, to: number): UnifiedLine[] {
  const lines: UnifiedLine[] = [];
  for (let number = from; number <= to; number += 1) {
    lines.push({
      kind: "context",
      text: `c${number}`,
      oldNumber: number,
      newNumber: number,
    });
  }
  return lines;
}
