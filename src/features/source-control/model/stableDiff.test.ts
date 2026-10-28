import { describe, expect, it } from "vitest";
import {
  pruneMap,
  reuseIfShallowEqual,
  reuseUnchangedById,
} from "./stableDiff";

describe("reuseIfShallowEqual", () => {
  it("keeps the previous object when every field is identical", () => {
    const blocks: unknown[] = [];
    const previous = { a: 1, blocks };
    expect(reuseIfShallowEqual(previous, { a: 1, blocks })).toBe(previous);
  });

  it("takes the next object when a field changed", () => {
    const next = { a: 1, blocks: [] };
    expect(reuseIfShallowEqual({ a: 1, blocks: [] }, next)).toBe(next);
    const widened = { a: 1, b: undefined };
    expect(
      reuseIfShallowEqual<Record<string, unknown>>({ a: 1 }, widened),
    ).toBe(widened);
  });
});

describe("reuseUnchangedById", () => {
  it("returns the previous array when nothing changed", () => {
    const previous = [
      { id: "a", n: 1 },
      { id: "b", n: 2 },
    ];
    expect(
      reuseUnchangedById(previous, [
        { id: "a", n: 1 },
        { id: "b", n: 2 },
      ]),
    ).toBe(previous);
  });

  it("keeps unchanged items when one item changes or the list grows", () => {
    const a = { id: "a", n: 1 };
    const previous = [a, { id: "b", n: 2 }];
    const merged = reuseUnchangedById(previous, [
      { id: "c", n: 0 },
      { id: "a", n: 1 },
      { id: "b", n: 3 },
    ]);
    expect(merged).not.toBe(previous);
    expect(merged[1]).toBe(a);
    expect(merged[2]).toEqual({ id: "b", n: 3 });
  });
});

describe("pruneMap", () => {
  it("keeps the same map when no keys are removed", () => {
    const map = new Map([["a", 1]]);
    expect(pruneMap(map, new Set(["a", "b"]))).toBe(map);
  });

  it("drops keys that are no longer present", () => {
    const map = new Map([
      ["a", 1],
      ["b", 2],
    ]);
    expect([...pruneMap(map, new Set(["b"]))]).toEqual([["b", 2]]);
  });
});
