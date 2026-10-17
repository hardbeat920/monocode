import { describe, expect, it } from "vitest";
import { isNumericColumn, parseDelimited } from "./csv";

describe("parseDelimited", () => {
  it("reads quoted fields with delimiters, escaped quotes and line breaks", () => {
    const text = 'name,note\r\n"Smith, J","said ""hi""\r\nthen left"\r\n';
    expect(parseDelimited(text, ",")).toEqual({
      rows: [
        ["name", "note"],
        ["Smith, J", 'said "hi"\r\nthen left'],
      ],
      truncated: false,
    });
  });

  it("drops a leading BOM and blank lines, and pads short rows", () => {
    expect(parseDelimited("\uFEFFa,b,c\n\n1\n2,3\n", ",").rows).toEqual([
      ["a", "b", "c"],
      ["1", "", ""],
      ["2", "3", ""],
    ]);
  });

  it("keeps empty fields and a last row without a line break", () => {
    expect(parseDelimited('a,,""\nx,y,z', ",").rows).toEqual([
      ["a", "", ""],
      ["x", "y", "z"],
    ]);
  });

  it("splits TSV on tabs only", () => {
    expect(parseDelimited("a\tb,c\n1\t2", "\t").rows).toEqual([
      ["a", "b,c"],
      ["1", "2"],
    ]);
  });

  it("stops at the limit and reports whether rows were left out", () => {
    expect(parseDelimited("a\nb\nc", ",", 2)).toEqual({
      rows: [["a"], ["b"]],
      truncated: true,
    });
    expect(parseDelimited("a\nb\n\n", ",", 2).truncated).toBe(false);
  });
});

describe("isNumericColumn", () => {
  it("treats a column of numbers, blanks allowed, as numeric", () => {
    const rows = [
      ["1,234.5", "2026-10-15", "", "-"],
      ["-3", "7", "", "4"],
      ["", "x", "", ""],
      ["12%", "8", "", "1e3"],
    ];
    expect(isNumericColumn(rows, 0)).toBe(true);
    expect(isNumericColumn(rows, 1)).toBe(false);
    expect(isNumericColumn(rows, 2)).toBe(false);
    expect(isNumericColumn(rows, 3)).toBe(false);
  });
});
