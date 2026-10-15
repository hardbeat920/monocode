import { describe, expect, it } from "vitest";
import { isJsonlPath, parseJsonl, summarizeJson } from "./jsonl";

describe("isJsonlPath", () => {
  it("matches JSON Lines and NDJSON files only", () => {
    expect(isJsonlPath("/repo/logs/events.jsonl")).toBe(true);
    expect(isJsonlPath("/repo/OUT.NDJSON")).toBe(true);
    expect(isJsonlPath("/repo/package.json")).toBe(false);
    expect(isJsonlPath("/repo/jsonl")).toBe(false);
  });
});

describe("parseJsonl", () => {
  it("keeps file line numbers, skips blank lines and keeps invalid lines", () => {
    const { records, total } = parseJsonl(
      '{"id":1}\r\n\n   \n{"id":2,\n[1,2]\n',
    );
    expect(total).toBe(3);
    expect(records.map((record) => record.line)).toEqual([1, 4, 5]);
    expect(records[0]).toMatchObject({ value: { id: 1 }, summary: "id: 1" });
    expect(records[1]).toMatchObject({ text: '{"id":2,' });
    expect("error" in records[1] && records[1].error).toBeTruthy();
    expect(records[2]).toMatchObject({ value: [1, 2], summary: "[1, 2]" });
  });

  it("parses only the first records of a long file and counts the rest", () => {
    const text = Array.from({ length: 25 }, (_, i) => `{"n":${i}}`).join("\n");
    const { records, total } = parseJsonl(text, 10);
    expect(records).toHaveLength(10);
    expect(records.at(-1)?.line).toBe(10);
    expect(total).toBe(25);
  });
});

describe("summarizeJson", () => {
  it("lists leading pairs with nested values reduced to their size", () => {
    expect(
      summarizeJson({
        type: "message",
        ok: true,
        tags: ["a"],
        meta: { a: 1, b: null },
        empty: {},
        none: null,
      }),
    ).toBe(
      'type: "message", ok: true, tags: [1 item], meta: {2 keys}, empty: {}, none: null',
    );
  });

  it("shortens long strings and stops at the summary budget", () => {
    const long = "x".repeat(100);
    expect(summarizeJson({ text: long })).toBe(`text: "${"x".repeat(60)}…"`);
    const wide = Object.fromEntries(
      Array.from({ length: 40 }, (_, i) => [`key${i}`, i]),
    );
    const summary = summarizeJson(wide);
    expect(summary.startsWith("key0: 0, key1: 1")).toBe(true);
    expect(summary.endsWith(", …")).toBe(true);
    expect(summary.length).toBeLessThan(170);
  });

  it("summarizes primitives and empty containers", () => {
    expect(summarizeJson("hi")).toBe('"hi"');
    expect(summarizeJson(42)).toBe("42");
    expect(summarizeJson({})).toBe("{}");
    expect(summarizeJson([])).toBe("[]");
  });
});
