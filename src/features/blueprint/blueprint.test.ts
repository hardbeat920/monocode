import { describe, expect, it } from "vitest";
import {
  parseBlueprint,
  serializeBlueprint,
  type Blueprint,
} from "./blueprint";

// The file is the source of truth the agent will read: what is drawn must
// come back identical, and a damaged file must refuse to load so it is
// never overwritten by an empty canvas.
describe("blueprint file", () => {
  const sample: Blueprint = {
    version: 1,
    nodes: [
      {
        id: "a",
        x: 10,
        y: 20,
        kind: "api",
        shape: "rectangle",
        line: "solid",
        title: "Orders",
        instructions: "REST",
        color: "#f472b6",
      },
      {
        id: "b",
        x: 200,
        y: 20,
        kind: "db",
        shape: "stadium",
        line: "dashed",
        title: "DB",
        instructions: "",
        color: "#fbbf24",
      },
    ],
    edges: [
      {
        id: "e",
        source: "a",
        target: "b",
        label: "reads",
        line: "dashed",
        color: "#ff0000",
      },
    ],
  };

  it("round-trips nodes, instructions and edge labels", () => {
    expect(parseBlueprint(serializeBlueprint(sample))).toEqual(sample);
  });

  it("reads files saved before shapes and lines existed as solid rectangles", () => {
    const { shape: _s, line: _l, ...old } = sample.nodes[1];
    const text = JSON.stringify({ ...sample, nodes: [old], edges: [] });
    const node = parseBlueprint(text).nodes[0];
    expect(node.shape).toBe("rectangle");
    expect(node.line).toBe("solid");
    const { line: _e, color: _c, ...oldEdge } = sample.edges[0];
    const edges = parseBlueprint(
      JSON.stringify({ ...sample, edges: [oldEdge] }),
    ).edges;
    expect(edges[0].line).toBe("solid");
    expect(edges[0].color).toBe("#e8f1ff");
  });

  it("falls back to General for an unknown kind", () => {
    const text = JSON.stringify({
      ...sample,
      nodes: [{ ...sample.nodes[0], kind: "wat" }],
      edges: [],
    });
    expect(parseBlueprint(text).nodes[0].kind).toBe("general");
  });

  it("rejects arrows that point to a missing box", () => {
    const text = JSON.stringify({
      ...sample,
      edges: [{ ...sample.edges[0], target: "gone" }],
    });
    expect(() => parseBlueprint(text)).toThrow(/does not exist/);
  });

  it("rejects files that are not blueprints", () => {
    expect(() => parseBlueprint("{}")).toThrow();
    expect(() => parseBlueprint("nope")).toThrow();
  });
});
