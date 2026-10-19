export const BLUEPRINT_KINDS = [
  { id: "screen", label: "Screen", color: "#60a5fa" },
  { id: "component", label: "Component", color: "#a78bfa" },
  { id: "service", label: "Service", color: "#34d399" },
  { id: "db", label: "DB", color: "#fbbf24" },
  { id: "api", label: "API", color: "#f472b6" },
  { id: "queue", label: "Queue", color: "#fb923c" },
  { id: "external", label: "External", color: "#94a3b8" },
  { id: "general", label: "General", color: "#e5e7eb" },
] as const;

export type BlueprintKind = (typeof BLUEPRINT_KINDS)[number]["id"];

/** Stadium: a rectangle with fully rounded ends (a "pill"). */
export const BLUEPRINT_SHAPES = [
  { id: "rectangle", label: "Rectangle" },
  { id: "stadium", label: "Stadium" },
] as const;

export type BlueprintShape = (typeof BLUEPRINT_SHAPES)[number]["id"];

export const BLUEPRINT_LINES = [
  { id: "solid", label: "Solid" },
  { id: "dashed", label: "Dashed" },
] as const;

export type BlueprintLine = (typeof BLUEPRINT_LINES)[number]["id"];

export type BlueprintNode = {
  id: string;
  x: number;
  y: number;
  kind: BlueprintKind;
  shape: BlueprintShape;
  line: BlueprintLine;
  title: string;
  instructions: string;
  color: string;
};

export type BlueprintEdge = {
  id: string;
  source: string;
  target: string;
  label: string;
  line: BlueprintLine;
  color: string;
};

export type Blueprint = {
  version: 1;
  nodes: BlueprintNode[];
  edges: BlueprintEdge[];
};

export const BLUEPRINT_FILE = ".monocode/blueprint.json";

/** Default arrow color: near-white, readable on the blue canvas. */
export const ARROW_COLOR = "#e8f1ff";

/** Default color of a box type; falls back to the General color. */
export function kindColor(kind: BlueprintKind): string {
  return BLUEPRINT_KINDS.find((k) => k.id === kind)?.color ?? "#e5e7eb";
}

/** File text for a blueprint: pretty-printed JSON with a trailing newline, so diffs stay readable. */
export function serializeBlueprint(blueprint: Blueprint): string {
  return `${JSON.stringify(blueprint, null, 2)}\n`;
}

/** Throws on anything that is not a blueprint, so a bad file is never overwritten. */
export function parseBlueprint(text: string): Blueprint {
  const raw = JSON.parse(text) as Partial<Blueprint>;
  if (
    raw.version !== 1 ||
    !Array.isArray(raw.nodes) ||
    !Array.isArray(raw.edges)
  ) {
    throw new Error("Not a blueprint file.");
  }
  const kinds = new Set<string>(BLUEPRINT_KINDS.map((k) => k.id));
  const nodes: BlueprintNode[] = raw.nodes.map((n) => {
    const kind = kinds.has(n.kind) ? n.kind : "general";
    return {
      id: String(n.id),
      x: Number(n.x) || 0,
      y: Number(n.y) || 0,
      kind,
      // Files saved before shapes existed are all rectangles.
      shape: n.shape === "stadium" ? "stadium" : "rectangle",
      line: n.line === "dashed" ? "dashed" : "solid",
      title: String(n.title ?? ""),
      instructions: String(n.instructions ?? ""),
      color: String(n.color || kindColor(kind)),
    };
  });
  const edges: BlueprintEdge[] = raw.edges.map((e) => ({
    id: String(e.id),
    source: String(e.source),
    target: String(e.target),
    label: String(e.label ?? ""),
    line: e.line === "dashed" ? "dashed" : "solid",
    color: String(e.color || ARROW_COLOR),
  }));
  // An arrow to a box that is not in the file (e.g. a hand- or agent-written
  // file) would break the canvas and the Implement brief: refuse the file.
  const ids = new Set(nodes.map((n) => n.id));
  const dangling = edges.find((e) => !ids.has(e.source) || !ids.has(e.target));
  if (dangling) {
    throw new Error(
      `Arrow ${dangling.id} points to a box that does not exist.`,
    );
  }
  return { version: 1, nodes, edges };
}
