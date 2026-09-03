import { describe, expect, it } from "vitest";
import { substringPositions } from "./fuzzy";

describe("substringPositions", () => {
  it("returns empty for a blank needle", () => {
    expect(substringPositions("monocode", "")).toEqual([]);
    expect(substringPositions("monocode", "   ")).toEqual([]);
  });

  it("matches case-insensitively", () => {
    expect(substringPositions("Fix/OMP-RPC-v2", "omp")).toEqual([4, 5, 6]);
  });

  it("covers every non-overlapping occurrence", () => {
    expect(substringPositions("aaa", "aa")).toEqual([0, 1]);
    expect(substringPositions("main main", "main")).toEqual([
      0, 1, 2, 3, 5, 6, 7, 8,
    ]);
  });

  it("returns empty when there is no match", () => {
    expect(substringPositions("main", "zzz")).toEqual([]);
  });
});
