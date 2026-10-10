// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, it } from "vitest";
import { plainEqual, useStableValue } from "./useStableValue";

(
  globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean }
).IS_REACT_ACT_ENVIRONMENT = true;

describe("plainEqual", () => {
  it("compares plain data structurally", () => {
    expect(plainEqual({ a: [1, { b: "x" }] }, { a: [1, { b: "x" }] })).toBe(
      true,
    );
    expect(plainEqual({ a: 1 }, { a: 1, b: undefined })).toBe(false);
    expect(plainEqual([1, 2], [2, 1])).toBe(false);
    expect(plainEqual([], {})).toBe(false);
    expect(plainEqual(null, {})).toBe(false);
  });

  it("compares Maps by entry and Sets by member", () => {
    expect(
      plainEqual(new Map([["a", { s: 1 }]]), new Map([["a", { s: 1 }]])),
    ).toBe(true);
    expect(
      plainEqual(new Map([["a", { s: 1 }]]), new Map([["a", { s: 2 }]])),
    ).toBe(false);
    expect(plainEqual(new Set(["a", "b"]), new Set(["b", "a"]))).toBe(true);
    expect(plainEqual(new Set(["a"]), new Set(["b"]))).toBe(false);
  });
});

describe("useStableValue", () => {
  it("keeps the first identity until the content changes", () => {
    const seen: string[][] = [];
    function Probe({ value }: { value: string[] }) {
      seen.push(useStableValue(value));
      return null;
    }
    const root = createRoot(document.createElement("div"));
    const first = ["a"];
    act(() => root.render(createElement(Probe, { value: first })));
    act(() => root.render(createElement(Probe, { value: ["a"] })));
    act(() => root.render(createElement(Probe, { value: ["b"] })));
    act(() => root.unmount());
    expect(seen[1]).toBe(first);
    expect(seen[2]).toEqual(["b"]);
  });
});
