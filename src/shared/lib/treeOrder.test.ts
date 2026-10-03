import { describe, expect, it } from "vitest";
import { flattenVisible } from "./treeOrder";

type Node = { id: string; kids?: Node[]; open?: boolean };

const tree: Node[] = [
  {
    id: "a",
    open: true,
    kids: [
      { id: "a/x", open: true, kids: [{ id: "a/x/1" }] },
      { id: "a/y", open: false, kids: [{ id: "a/y/1" }] },
      { id: "a/2" },
    ],
  },
  { id: "b" },
];

const children = (node: Node) => (node.open ? (node.kids ?? []) : null);

describe("flattenVisible", () => {
  it("walks pre-order, skipping collapsed subtrees", () => {
    expect(flattenVisible(tree, { children, id: (n) => n.id })).toEqual([
      "a",
      "a/x",
      "a/x/1",
      "a/y",
      "a/2",
      "b",
    ]);
  });

  it("descends into excluded nodes", () => {
    expect(
      flattenVisible(tree, {
        children,
        id: (n) => n.id,
        include: (n) => !n.kids,
      }),
    ).toEqual(["a/x/1", "a/2", "b"]);
  });

  it("returns nothing for no roots", () => {
    expect(flattenVisible([], { children, id: (n: Node) => n.id })).toEqual([]);
  });
});
