import { describe, expect, it } from "vitest";
import { mergeOrderedSubset } from "./reorder";

type Item = { id: string; value?: string };

describe("mergeOrderedSubset", () => {
  it("reorders a project-scoped subset without moving hidden items", () => {
    const items: Item[] = [
      { id: "project-a-1" },
      { id: "project-b-1" },
      { id: "project-a-2" },
    ];

    expect(
      mergeOrderedSubset(items, [items[2], items[0]]).map((item) => item.id),
    ).toEqual(["project-a-2", "project-b-1", "project-a-1"]);
  });

  it("keeps updated subset entries", () => {
    const items: Item[] = [{ id: "a" }, { id: "hidden" }, { id: "b" }];
    const next = mergeOrderedSubset(items, [
      { id: "b", value: "updated" },
      items[0],
    ]);

    expect(next).toEqual([
      { id: "b", value: "updated" },
      { id: "hidden" },
      { id: "a" },
    ]);
  });

  it("rejects duplicate or unknown subset ids", () => {
    const items: Item[] = [{ id: "a" }, { id: "b" }];

    expect(mergeOrderedSubset(items, [items[0], items[0]])).toBe(items);
    expect(mergeOrderedSubset(items, [{ id: "missing" }])).toBe(items);
  });
});
