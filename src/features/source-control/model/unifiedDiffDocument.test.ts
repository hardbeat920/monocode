import { expect, it } from "vitest";
import { buildUnifiedFile } from "./unifiedDiff";
import {
  DIFF_FILE_HEADER_PX,
  ReviewDocumentBuilder,
  visualColumns,
  type UnifiedDiffFileModel,
} from "./unifiedDiffDocument";

function file(id: string): UnifiedDiffFileModel {
  const old = Array.from({ length: 100 }, (_, index) => `line ${index}`);
  const next = old.map((line, index) =>
    [12, 60].includes(index) ? `${line} changed` : line,
  );
  const diff = buildUnifiedFile(old.join("\n"), next.join("\n"));
  return {
    id,
    path: id,
    label: id,
    additions: diff.additions,
    deletions: diff.deletions,
    blocks: diff.blocks,
    canStageHunk: true,
  };
}

it("keeps hunk targets for collapsed files without materializing their body rows", () => {
  const builder = new ReviewDocumentBuilder();
  const files = [file("a"), file("b")];
  const document = builder.build(files, new Set(), true);
  expect(document.rows.map((row) => row.type)).toEqual(["file", "file"]);
  expect(document.totalHeight).toBe(DIFF_FILE_HEADER_PX * 2);
  expect(document.hunks).toHaveLength(4);
  expect(document.hunks.every((hunk) => hunk.rowIndex === null)).toBe(true);
  const expanded = builder.build(files, new Set(["b"]), true);
  expect(
    expanded.hunks.slice(0, 2).every((hunk) => hunk.rowIndex === null),
  ).toBe(true);
  expect(
    expanded.hunks
      .slice(2)
      .every((hunk) => expanded.rows[hunk.rowIndex!].type === "hunk"),
  ).toBe(true);
});

it("reuses source rows when another file loads and preserves original staging positions", () => {
  const builder = new ReviewDocumentBuilder();
  const first = file("first");
  const before = builder.build([first], new Set([first.id]), true);
  const after = builder.build(
    [{ ...first }, file("second")],
    new Set(["first", "second"]),
    true,
  );
  const oldRows = before.rows.filter((row) => row.type === "line");
  const newRows = after.rows.filter(
    (row) => row.fileId === "first" && row.type === "line",
  );
  expect(oldRows.length).toBeGreaterThan(0);
  oldRows.forEach((row, index) => expect(row).toBe(newRows[index]));
  for (const row of oldRows)
    if (row.type === "line" && row.stagePos != null) {
      expect(row.stagePos).toBe(row.line.pos);
      expect(first.blocks.some((block) => block.lines.includes(row.line))).toBe(
        true,
      );
    }
});

it("omits unmodified sections while retaining context around each navigable hunk", () => {
  const builder = new ReviewDocumentBuilder();
  const model = file("a");
  const document = builder.build([model], new Set(["a"]), true);
  expect(model.blocks.some((block) => block.kind === "fold")).toBe(true);
  expect([...new Set(document.rows.map((row) => row.type))]).toEqual([
    "file",
    "hunk",
    "line",
  ]);
  expect(document.hunks).toHaveLength(2);
  const context = document.rows.flatMap((row) =>
    row.type === "line" && row.line.kind === "context"
      ? [row.line.oldNumber]
      : [],
  );
  expect(context).toEqual([10, 11, 12, 14, 15, 16, 58, 59, 60, 62, 63, 64]);
});

it("measures tab stops, wide glyphs, and combining marks for stable horizontal scrolling", () => {
  expect(visualColumns("a\t中x")).toBe(7);
  expect(visualColumns("日本語")).toBe(6);
  expect(visualColumns("e\u0301")).toBe(1);
  expect(visualColumns("a🪴b")).toBe(4);
});
