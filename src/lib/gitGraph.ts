/**
 * Copyright (c) Microsoft Corporation.
 *
 * Portions derived from Visual Studio Code
 * (`src/vs/workbench/contrib/scm/browser/scmHistory.ts`).
 *
 * MIT License
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in all
 * copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 *
 * Git graph layout + per-row SVG paths. Each row is an independent 22px SVG.
 * Previous row `outputSwimlanes` become the next row's `inputSwimlanes`.
 * Curves are SVG arcs, not cubics.
 */

export type GraphRef = {
  name: string;
  kind: string;
  color?: string;
};

export type GraphCommit = {
  sha: string;
  parents: string[];
  head?: boolean;
  refs?: readonly { name: string; kind: string }[];
};

export type Swimlane = {
  id: string;
  color: string;
};

export type HistoryItemViewModel = {
  sha: string;
  parentIds: string[];
  kind: "HEAD" | "node";
  inputSwimlanes: Swimlane[];
  outputSwimlanes: Swimlane[];
  refs: GraphRef[];
};

export type GraphPath = {
  d: string;
  color: string;
  strokeWidth: number;
};

export type GraphCircle = {
  cx: number;
  cy: number;
  r: number;
  strokeWidth: number;
  fill?: string;
};

export type HistoryItemGraph = {
  width: number;
  height: number;
  paths: GraphPath[];
  circles: GraphCircle[];
  kind: "HEAD" | "node";
  circleColor: string;
};

export const SWIMLANE_HEIGHT = 22;
export const SWIMLANE_WIDTH = 11;
const SWIMLANE_CURVE_RADIUS = 5;
const CIRCLE_RADIUS = 4;
const CIRCLE_STROKE_WIDTH = 2;

export const GRAPH_ROW_PX = SWIMLANE_HEIGHT;

/** Lane colors, cycled as new branches appear. */
export const COLOR_REGISTRY = [
  "#FFB000",
  "#DC267F",
  "#994F00",
  "#40B0A6",
  "#B66DFF",
] as const;

/** Current branch. */
export const HISTORY_ITEM_REF_COLOR = "#75BEFF";
/** Upstream of the current branch. */
export const HISTORY_ITEM_REMOTE_REF_COLOR = "#B180D7";
/** Fallback when a node has no swimlane color. */
const HISTORY_ITEM_REF_FALLBACK = HISTORY_ITEM_REF_COLOR;

export const GRAPH_LANE_COLORS = COLOR_REGISTRY;

function rot(index: number, modulo: number): number {
  return ((index % modulo) + modulo) % modulo;
}

function refId(ref: { name: string; kind: string }): string {
  return `${ref.kind}:${ref.name}`;
}

function buildColorMap(
  commits: GraphCommit[],
): Map<string, string | undefined> {
  const colorMap = new Map<string, string | undefined>();
  const head = commits.find((commit) => commit.head);
  const local = head?.refs?.find((ref) => ref.kind === "local");
  const remote = local
    ? head?.refs?.find(
        (ref) =>
          ref.kind === "remote" &&
          (ref.name === local.name || ref.name.endsWith(`/${local.name}`)),
      )
    : undefined;
  const currentLocalId = local ? refId(local) : undefined;
  const currentRemoteId = remote ? refId(remote) : undefined;

  if (currentLocalId) {
    colorMap.set(currentLocalId, HISTORY_ITEM_REF_COLOR);
    if (currentRemoteId) {
      colorMap.set(currentRemoteId, HISTORY_ITEM_REMOTE_REF_COLOR);
    }
  }

  for (const commit of commits) {
    for (const ref of commit.refs ?? []) {
      const id = refId(ref);
      if (!colorMap.has(id)) colorMap.set(id, undefined);
    }
  }

  return colorMap;
}

function getLabelColorIdentifier(
  refs: readonly { name: string; kind: string }[] | undefined,
  colorMap: Map<string, string | undefined>,
): string | undefined {
  for (const ref of refs ?? []) {
    const color = colorMap.get(refId(ref));
    if (color !== undefined) return color;
  }
  return undefined;
}

function compareRefs(
  ref1: GraphRef,
  ref2: GraphRef,
  currentLocalId?: string,
  currentRemoteId?: string,
): number {
  const order = (ref: GraphRef) => {
    const id = refId(ref);
    if (currentLocalId && id === currentLocalId) return 1;
    if (currentRemoteId && id === currentRemoteId) return 2;
    if (ref.color !== undefined) return 4;
    return 99;
  };
  return order(ref1) - order(ref2);
}

function currentRefIds(commits: GraphCommit[]): {
  local?: string;
  remote?: string;
} {
  const head = commits.find((commit) => commit.head);
  const local = head?.refs?.find((ref) => ref.kind === "local");
  const remote = local
    ? head?.refs?.find(
        (ref) =>
          ref.kind === "remote" &&
          (ref.name === local.name || ref.name.endsWith(`/${local.name}`)),
      )
    : undefined;
  return {
    local: local ? refId(local) : undefined,
    remote: remote ? refId(remote) : undefined,
  };
}

/** Assign swimlanes for `git log --topo-order` (newest first). */
export function layoutGitGraph(commits: GraphCommit[]): HistoryItemViewModel[] {
  const colorMap = buildColorMap(commits);
  const current = currentRefIds(commits);
  let colorIndex = -1;
  const viewModels: HistoryItemViewModel[] = [];

  for (const commit of commits) {
    const kind = commit.head ? "HEAD" : "node";
    const inputSwimlanes = (
      viewModels[viewModels.length - 1]?.outputSwimlanes ?? []
    ).map((lane) => ({ ...lane }));
    const outputSwimlanes: Swimlane[] = [];
    const parentIds = commit.parents.filter(Boolean);
    let firstParentAdded = false;

    if (parentIds.length > 0) {
      for (const node of inputSwimlanes) {
        if (node.id === commit.sha) {
          if (!firstParentAdded) {
            outputSwimlanes.push({
              id: parentIds[0] ?? node.id,
              color:
                getLabelColorIdentifier(commit.refs, colorMap) ?? node.color,
            });
            firstParentAdded = true;
          }
          continue;
        }
        outputSwimlanes.push({ ...node });
      }
    }

    for (let i = firstParentAdded ? 1 : 0; i < parentIds.length; i += 1) {
      let color: string | undefined;
      if (i === 0) {
        color = getLabelColorIdentifier(commit.refs, colorMap);
      } else {
        const parent = commits.find((item) => item.sha === parentIds[i]);
        color = parent
          ? getLabelColorIdentifier(parent.refs, colorMap)
          : undefined;
      }
      if (!color) {
        colorIndex = rot(colorIndex + 1, COLOR_REGISTRY.length);
        color = COLOR_REGISTRY[colorIndex] ?? COLOR_REGISTRY[0];
      }
      outputSwimlanes.push({
        id: parentIds[i] ?? "",
        color,
      });
    }

    const refs: GraphRef[] = (commit.refs ?? []).map((ref) => {
      const id = refId(ref);
      let color = colorMap.get(id);
      if (colorMap.has(id) && color === undefined) {
        const inputIndex = inputSwimlanes.findIndex(
          (node) => node.id === commit.sha,
        );
        const circleIndex =
          inputIndex !== -1 ? inputIndex : inputSwimlanes.length;
        color =
          circleIndex < outputSwimlanes.length
            ? outputSwimlanes[circleIndex]?.color
            : circleIndex < inputSwimlanes.length
              ? inputSwimlanes[circleIndex]?.color
              : HISTORY_ITEM_REF_FALLBACK;
      }
      return { name: ref.name, kind: ref.kind, color };
    });
    refs.sort((a, b) => compareRefs(a, b, current.local, current.remote));

    viewModels.push({
      sha: commit.sha,
      parentIds,
      kind,
      inputSwimlanes,
      outputSwimlanes,
      refs,
    });
  }

  return viewModels;
}

function findLastIndex(nodes: Swimlane[], id: string): number {
  for (let i = nodes.length - 1; i >= 0; i -= 1) {
    if (nodes[i]?.id === id) return i;
  }
  return -1;
}

export function historyItemIndex(viewModel: HistoryItemViewModel): number {
  const inputIndex = viewModel.inputSwimlanes.findIndex(
    (node) => node.id === viewModel.sha,
  );
  return inputIndex !== -1 ? inputIndex : viewModel.inputSwimlanes.length;
}

/** One row of graph paths and circles. */
export function historyItemGraph(
  viewModel: HistoryItemViewModel,
): HistoryItemGraph {
  const { parentIds, inputSwimlanes, outputSwimlanes, kind } = viewModel;
  const inputIndex = inputSwimlanes.findIndex(
    (node) => node.id === viewModel.sha,
  );
  const circleIndex = inputIndex !== -1 ? inputIndex : inputSwimlanes.length;
  const circleColor =
    circleIndex < outputSwimlanes.length
      ? (outputSwimlanes[circleIndex]?.color ?? HISTORY_ITEM_REF_FALLBACK)
      : circleIndex < inputSwimlanes.length
        ? (inputSwimlanes[circleIndex]?.color ?? HISTORY_ITEM_REF_FALLBACK)
        : HISTORY_ITEM_REF_FALLBACK;

  const paths: GraphPath[] = [];
  const pushPath = (d: string, color: string) => {
    paths.push({ d, color, strokeWidth: 1 });
  };

  let outputSwimlaneIndex = 0;
  for (let index = 0; index < inputSwimlanes.length; index += 1) {
    const color = inputSwimlanes[index]?.color ?? HISTORY_ITEM_REF_FALLBACK;

    if (inputSwimlanes[index]?.id === viewModel.sha) {
      if (index !== circleIndex) {
        pushPath(
          [
            `M ${SWIMLANE_WIDTH * (index + 1)} 0`,
            `A ${SWIMLANE_WIDTH} ${SWIMLANE_WIDTH} 0 0 1 ${SWIMLANE_WIDTH * index} ${SWIMLANE_WIDTH}`,
            `H ${SWIMLANE_WIDTH * (circleIndex + 1)}`,
          ].join(" "),
          color,
        );
      } else {
        outputSwimlaneIndex += 1;
      }
    } else if (
      outputSwimlaneIndex < outputSwimlanes.length &&
      inputSwimlanes[index]?.id === outputSwimlanes[outputSwimlaneIndex]?.id
    ) {
      if (index === outputSwimlaneIndex) {
        pushPath(
          `M ${SWIMLANE_WIDTH * (index + 1)} 0 V ${SWIMLANE_HEIGHT}`,
          color,
        );
      } else {
        pushPath(
          [
            `M ${SWIMLANE_WIDTH * (index + 1)} 0`,
            `V 6`,
            `A ${SWIMLANE_CURVE_RADIUS} ${SWIMLANE_CURVE_RADIUS} 0 0 1 ${SWIMLANE_WIDTH * (index + 1) - SWIMLANE_CURVE_RADIUS} ${SWIMLANE_HEIGHT / 2}`,
            `H ${SWIMLANE_WIDTH * (outputSwimlaneIndex + 1) + SWIMLANE_CURVE_RADIUS}`,
            `A ${SWIMLANE_CURVE_RADIUS} ${SWIMLANE_CURVE_RADIUS} 0 0 0 ${SWIMLANE_WIDTH * (outputSwimlaneIndex + 1)} ${SWIMLANE_HEIGHT / 2 + SWIMLANE_CURVE_RADIUS}`,
            `V ${SWIMLANE_HEIGHT}`,
          ].join(" "),
          color,
        );
      }
      outputSwimlaneIndex += 1;
    }
  }

  for (let i = 1; i < parentIds.length; i += 1) {
    const parentId = parentIds[i];
    if (!parentId) continue;
    const parentOutputIndex = findLastIndex(outputSwimlanes, parentId);
    if (parentOutputIndex === -1) continue;
    const color =
      outputSwimlanes[parentOutputIndex]?.color ?? HISTORY_ITEM_REF_FALLBACK;
    pushPath(
      [
        `M ${SWIMLANE_WIDTH * parentOutputIndex} ${SWIMLANE_HEIGHT / 2}`,
        `A ${SWIMLANE_WIDTH} ${SWIMLANE_WIDTH} 0 0 1 ${SWIMLANE_WIDTH * (parentOutputIndex + 1)} ${SWIMLANE_HEIGHT}`,
        `M ${SWIMLANE_WIDTH * parentOutputIndex} ${SWIMLANE_HEIGHT / 2}`,
        `H ${SWIMLANE_WIDTH * (circleIndex + 1)} `,
      ].join(" "),
      color,
    );
  }

  if (inputIndex !== -1) {
    pushPath(
      `M ${SWIMLANE_WIDTH * (circleIndex + 1)} 0 V ${SWIMLANE_HEIGHT / 2}`,
      inputSwimlanes[inputIndex]?.color ?? HISTORY_ITEM_REF_FALLBACK,
    );
  }

  if (parentIds.length > 0) {
    pushPath(
      `M ${SWIMLANE_WIDTH * (circleIndex + 1)} ${SWIMLANE_HEIGHT / 2} V ${SWIMLANE_HEIGHT}`,
      circleColor,
    );
  }

  const cx = SWIMLANE_WIDTH * (circleIndex + 1);
  const cy = SWIMLANE_WIDTH;
  const circles: GraphCircle[] = [];
  if (kind === "HEAD") {
    circles.push({
      cx,
      cy,
      r: CIRCLE_RADIUS + 3,
      strokeWidth: CIRCLE_STROKE_WIDTH,
      fill: circleColor,
    });
    circles.push({
      cx,
      cy,
      r: CIRCLE_STROKE_WIDTH,
      strokeWidth: CIRCLE_RADIUS,
    });
  } else if (parentIds.length > 1) {
    circles.push({
      cx,
      cy,
      r: CIRCLE_RADIUS + 2,
      strokeWidth: CIRCLE_STROKE_WIDTH,
      fill: circleColor,
    });
    circles.push({
      cx,
      cy,
      r: CIRCLE_RADIUS - 1,
      strokeWidth: CIRCLE_STROKE_WIDTH,
      fill: circleColor,
    });
  } else {
    circles.push({
      cx,
      cy,
      r: CIRCLE_RADIUS + 1,
      strokeWidth: CIRCLE_STROKE_WIDTH,
      fill: circleColor,
    });
  }

  return {
    width:
      SWIMLANE_WIDTH *
      (Math.max(inputSwimlanes.length, outputSwimlanes.length, 1) + 1),
    height: SWIMLANE_HEIGHT,
    paths,
    circles,
    kind,
    circleColor,
  };
}
