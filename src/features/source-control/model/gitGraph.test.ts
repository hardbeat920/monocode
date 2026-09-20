import { describe, expect, it } from "vitest";
import {
  COLOR_REGISTRY,
  HISTORY_ITEM_REF_COLOR,
  historyItemGraph,
  layoutGitGraph,
  SWIMLANE_HEIGHT,
  SWIMLANE_WIDTH,
} from "./gitGraph";

describe("layoutGitGraph", () => {
  it("keeps a linear history on a single swimlane", () => {
    const rows = layoutGitGraph([
      { sha: "c", parents: ["b"] },
      { sha: "b", parents: ["a"] },
      { sha: "a", parents: [] },
    ]);
    expect(rows.map((row) => row.outputSwimlanes.map((lane) => lane.id))).toEqual([
      ["b"],
      ["a"],
      [],
    ]);
    expect(rows.map((row) => row.inputSwimlanes.map((lane) => lane.id))).toEqual([
      [],
      ["b"],
      ["a"],
    ]);
    expect(rows[0]?.outputSwimlanes[0]?.color).toBe(COLOR_REGISTRY[0]);
  });

  it("opens a new swimlane for a merge parent and curves extra copies into the node", () => {
    const rows = layoutGitGraph([
      { sha: "M", parents: ["A", "F"] },
      { sha: "A", parents: ["B"] },
      { sha: "F", parents: ["B"] },
      { sha: "B", parents: [] },
    ]);
    expect(rows.map((row) => [row.sha, row.outputSwimlanes.map((l) => l.id)])).toEqual([
      ["M", ["A", "F"]],
      ["A", ["B", "F"]],
      ["F", ["B", "B"]],
      ["B", []],
    ]);
    expect(rows[0]?.outputSwimlanes.map((lane) => lane.color)).toEqual([
      COLOR_REGISTRY[0],
      COLOR_REGISTRY[1],
    ]);
  });

  it("forks a second child onto a new swimlane while the first lane continues", () => {
    const rows = layoutGitGraph([
      { sha: "C", parents: ["B"] },
      { sha: "D", parents: ["B"] },
      { sha: "B", parents: [] },
    ]);
    expect(rows.map((row) => [row.sha, row.outputSwimlanes.map((l) => l.id)])).toEqual([
      ["C", ["B"]],
      ["D", ["B", "B"]],
      ["B", []],
    ]);
  });

  it("keeps a through-lane vertical across a stash index commit", () => {
    const rows = layoutGitGraph([
      { sha: "A", parents: ["M"], head: true, refs: [{ name: "main", kind: "local" }] },
      { sha: "S", parents: ["W", "I"], refs: [{ name: "stash", kind: "local" }] },
      { sha: "I", parents: ["W"] },
      { sha: "W", parents: ["F"], refs: [{ name: "feature", kind: "local" }] },
      { sha: "F", parents: ["M"] },
      { sha: "M", parents: [] },
    ]);
    expect(rows.map((row) => [row.sha, row.outputSwimlanes.map((l) => l.id)])).toEqual([
      ["A", ["M"]],
      ["S", ["M", "W", "I"]],
      ["I", ["M", "W", "W"]],
      ["W", ["M", "F"]],
      ["F", ["M", "M"]],
      ["M", []],
    ]);
    const indexRow = historyItemGraph(rows[2]!);
    expect(indexRow.paths.map((path) => path.d)).toContain(
      `M ${SWIMLANE_WIDTH * 2} 0 V ${SWIMLANE_HEIGHT}`,
    );
    const join = historyItemGraph(rows[5]!);
    expect(join.paths.map((path) => path.d)).toContain(
      `M ${SWIMLANE_WIDTH * 2} 0 A ${SWIMLANE_WIDTH} ${SWIMLANE_WIDTH} 0 0 1 ${SWIMLANE_WIDTH} ${SWIMLANE_WIDTH} H ${SWIMLANE_WIDTH}`,
    );
  });

  it("colors the current branch with the history-item ref color", () => {
    const rows = layoutGitGraph([
      {
        sha: "c",
        parents: ["b"],
        head: true,
        refs: [{ name: "main", kind: "local" }],
      },
      { sha: "b", parents: [] },
    ]);
    expect(rows[0]?.kind).toBe("HEAD");
    expect(rows[0]?.outputSwimlanes[0]?.color).toBe(HISTORY_ITEM_REF_COLOR);
    expect(rows[0]?.refs[0]?.color).toBe(HISTORY_ITEM_REF_COLOR);
  });
});

describe("historyItemGraph", () => {
  it("draws a vertical trunk through a linear row", () => {
    const [row] = layoutGitGraph([
      { sha: "c", parents: ["b"] },
      { sha: "b", parents: ["a"] },
    ]);
    expect(row).toBeDefined();
    const graph = historyItemGraph(row!);
    expect(graph.height).toBe(SWIMLANE_HEIGHT);
    expect(graph.width).toBe(SWIMLANE_WIDTH * 2);
    expect(graph.paths.map((path) => path.d)).toEqual([
      `M ${SWIMLANE_WIDTH} ${SWIMLANE_HEIGHT / 2} V ${SWIMLANE_HEIGHT}`,
    ]);
    expect(graph.circles).toEqual([
      {
        cx: SWIMLANE_WIDTH,
        cy: SWIMLANE_WIDTH,
        r: 5,
        strokeWidth: 2,
        fill: COLOR_REGISTRY[0],
      },
    ]);
  });

  it("draws merge-out and merge-in arcs", () => {
    const rows = layoutGitGraph([
      { sha: "M", parents: ["A", "F"] },
      { sha: "A", parents: ["B"] },
      { sha: "F", parents: ["B"] },
      { sha: "B", parents: [] },
    ]);
    const merge = historyItemGraph(rows[0]!);
    expect(merge.circles[0]?.r).toBe(6);
    expect(merge.paths.map((path) => path.d)).toEqual([
      `M ${SWIMLANE_WIDTH} ${SWIMLANE_HEIGHT / 2} A ${SWIMLANE_WIDTH} ${SWIMLANE_WIDTH} 0 0 1 ${SWIMLANE_WIDTH * 2} ${SWIMLANE_HEIGHT} M ${SWIMLANE_WIDTH} ${SWIMLANE_HEIGHT / 2} H ${SWIMLANE_WIDTH} `,
      `M ${SWIMLANE_WIDTH} ${SWIMLANE_HEIGHT / 2} V ${SWIMLANE_HEIGHT}`,
    ]);

    const join = historyItemGraph(rows[3]!);
    expect(join.paths.map((path) => path.d)).toEqual([
      `M ${SWIMLANE_WIDTH * 2} 0 A ${SWIMLANE_WIDTH} ${SWIMLANE_WIDTH} 0 0 1 ${SWIMLANE_WIDTH} ${SWIMLANE_WIDTH} H ${SWIMLANE_WIDTH}`,
      `M ${SWIMLANE_WIDTH} 0 V ${SWIMLANE_HEIGHT / 2}`,
    ]);
  });

  it("draws a pass-through lane beside a new branch tip", () => {
    const rows = layoutGitGraph([
      { sha: "C", parents: ["B"] },
      { sha: "D", parents: ["B"] },
      { sha: "B", parents: [] },
    ]);
    const branchTip = historyItemGraph(rows[1]!);
    expect(branchTip.paths.map((path) => path.d)).toEqual([
      `M ${SWIMLANE_WIDTH} 0 V ${SWIMLANE_HEIGHT}`,
      `M ${SWIMLANE_WIDTH * 2} ${SWIMLANE_HEIGHT / 2} V ${SWIMLANE_HEIGHT}`,
    ]);
  });

  it("draws elbow arcs when a through-lane shifts left", () => {
    const rows = layoutGitGraph([
      { sha: "M", parents: ["A", "F"] },
      { sha: "C", parents: ["X"] },
      { sha: "A", parents: ["B"] },
      { sha: "F", parents: ["B"] },
      { sha: "B", parents: ["R"] },
      { sha: "X", parents: ["R"] },
      { sha: "R", parents: [] },
    ]);
    expect(rows[4]?.inputSwimlanes.map((lane) => lane.id)).toEqual(["B", "B", "X"]);
    expect(rows[4]?.outputSwimlanes.map((lane) => lane.id)).toEqual(["R", "X"]);
    const shifted = historyItemGraph(rows[4]!);
    expect(shifted.paths.map((path) => path.d)).toContain(
      [
        `M ${SWIMLANE_WIDTH * 3} 0`,
        `V 6`,
        `A 5 5 0 0 1 ${SWIMLANE_WIDTH * 3 - 5} ${SWIMLANE_HEIGHT / 2}`,
        `H ${SWIMLANE_WIDTH * 2 + 5}`,
        `A 5 5 0 0 0 ${SWIMLANE_WIDTH * 2} ${SWIMLANE_HEIGHT / 2 + 5}`,
        `V ${SWIMLANE_HEIGHT}`,
      ].join(" "),
    );
  });

  it("draws HEAD as an outer disc plus cutout inner circle", () => {
    const [row] = layoutGitGraph([
      { sha: "c", parents: ["b"], head: true },
    ]);
    const graph = historyItemGraph(row!);
    expect(graph.kind).toBe("HEAD");
    expect(graph.circles.map((circle) => [circle.r, circle.strokeWidth, circle.fill])).toEqual([
      [7, 2, COLOR_REGISTRY[0]],
      [2, 4, undefined],
    ]);
  });
});
