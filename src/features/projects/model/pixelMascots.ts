import { PROJECT_MASCOTS } from "./projectMascots";

/**
 * High-fidelity project mascots: the project's original 8×8 front sprites, styled
 * like the mascot studio's portraits — soft pastel blocks with a shallow
 * extruded edge, catch-lights on the top-left, accent bands and glossy eyes.
 *
 * Figures sit on a 16-unit grid, 1.5 units per sprite cell, centered with a
 * margin so they read at the same weight as the rest of the chrome.
 */
export const PIXEL_GRID = 16;

const CELL = 1.5;
/** Inset that centers the 8-cell sprite on the grid. */
const INSET = 2;
const SIZE = 8;

type Cell = [x: number, y: number];
/** A detail rect in sprite cells. */
type Detail = [x: number, y: number, w: number, h: number, fill: string];

type Figure = {
  /** Eye cells (holes in the sprite) for the rest and talk frames. */
  eyes: { rest: readonly Cell[]; talk: readonly Cell[] };
  /** Overrides the body tone per cell; `base` is the project color. */
  material?: (x: number, y: number, base: Hsl) => Hsl;
  /** Painted on top of the faces, per frame. */
  details?: { rest?: readonly Detail[]; talk?: readonly Detail[] };
};

type Hsl = readonly [h: number, s: number, l: number];

const same = (eyes: readonly Cell[]) => ({ rest: eyes, talk: eyes });

const FIGURES: Record<string, Figure> = {
  invader: {
    eyes: same([
      [2, 2],
      [5, 2],
    ]),
  },
  ghost: {
    eyes: {
      rest: [
        [2, 2],
        [5, 2],
      ],
      talk: [
        [1, 2],
        [4, 2],
      ],
    },
    material: (_x, y, base) => (y >= 6 ? tone(base, -0.08) : base),
  },
  robot: {
    eyes: same([
      [2, 2],
      [5, 2],
    ]),
    material: (_x, y, base) =>
      y === 0
        ? parse("#f0d9b1")
        : y === 2 || y === 3
          ? tone(base, 0.26)
          : y === 4
            ? tone(base, -0.24)
            : base,
    details: {
      rest: [
        [2.5, 4.25, 0.4, 0.32, "#efcf92"],
        [3.2, 4.25, 0.7, 0.32, "#82aaa2"],
      ],
    },
  },
  cat: {
    eyes: same([
      [1, 3],
      [6, 3],
    ]),
    material: (x, y, base) =>
      (x === 2 || x === 5) && y === 1
        ? parse("#d78f8b")
        : y <= 1
          ? tone(base, -0.03)
          : y === 5 || y === 6
            ? tone(base, 0.07)
            : base,
    details: {
      rest: [[3.7, 4.75, 0.28, 0.25, "#876451"]],
      talk: [[3.7, 4.75, 0.28, 0.25, "#876451"]],
    },
  },
  skull: {
    eyes: same([
      [2, 2],
      [5, 2],
    ]),
  },
  crab: {
    eyes: same([
      [2, 3],
      [5, 3],
    ]),
    material: (_x, y, base) => (y <= 1 ? tone(base, 0.05) : base),
  },
  mushroom: {
    eyes: {
      rest: [
        [2, 3],
        [5, 3],
      ],
      talk: [
        [2, 4],
        [5, 4],
      ],
    },
    material: (_x, y, base) => (y >= 5 ? parse("#f1e3be") : base),
    details: {
      rest: [
        [3.0, 0.42, 0.6, 0.35, "rgba(255,248,226,0.66)"],
        [1.65, 1.4, 0.5, 0.38, "rgba(255,248,226,0.66)"],
        [5.5, 1.7, 0.35, 0.4, "rgba(255,248,226,0.66)"],
      ],
      talk: [
        [3.0, 1.42, 0.6, 0.35, "rgba(255,248,226,0.66)"],
        [1.65, 2.4, 0.5, 0.38, "rgba(255,248,226,0.66)"],
        [5.5, 2.7, 0.35, 0.4, "rgba(255,248,226,0.66)"],
      ],
    },
  },
  rocket: {
    eyes: same([
      [3, 2],
      [4, 2],
    ]),
    material: (_x, y, base) =>
      y >= 6 ? parse("#f9c081") : y === 5 ? tone(base, -0.25) : base,
  },
  dino: {
    eyes: same([[5, 1]]),
    material: (x, y, base) => (y === 2 && x >= 5 ? parse("#ece0ae") : base),
  },
  frog: {
    eyes: {
      rest: [
        [1, 2],
        [6, 2],
      ],
      talk: [
        [1, 1],
        [6, 1],
      ],
    },
    material: (_x, y, base) => (y === 4 ? tone(parse("#f5e7ad"), -0.03) : base),
  },
};

const SPRITES = Object.fromEntries(
  PROJECT_MASCOTS.map((mascot) => [mascot.name, mascot]),
);

export const PIXEL_MASCOT_NAMES: readonly string[] = PROJECT_MASCOTS.map(
  (mascot) => mascot.name,
);

/** The 8×8 front rows for a mascot, `#` filled; unknown names get the ghost. */
export function pixelSprite(name: string, talking = false): readonly string[] {
  const mascot = SPRITES[name] ?? SPRITES.ghost;
  return talking ? mascot.talk : mascot.rest;
}

function figure(name: string): Figure {
  return FIGURES[name] ?? FIGURES.ghost;
}

/** Eye cells for a mascot frame, in sprite cells. */
export function pixelEyes(name: string, talking = false): readonly Cell[] {
  const { eyes } = figure(SPRITES[name] ? name : "ghost");
  return talking ? eyes.talk : eyes.rest;
}

const FALLBACK: Hsl = [280, 60, 66];

function parse(color: string): Hsl {
  const hslMatch = color.match(
    /hsl\(\s*([\d.]+)[\s,]+([\d.]+)%[\s,]+([\d.]+)%/,
  );
  if (hslMatch) {
    return [Number(hslMatch[1]), Number(hslMatch[2]), Number(hslMatch[3])];
  }
  const hex = color.match(/^#([0-9a-f]{6})$/i);
  if (!hex) return FALLBACK;
  const n = parseInt(hex[1], 16);
  const [r, g, b] = [(n >> 16) & 255, (n >> 8) & 255, n & 255].map(
    (v) => v / 255,
  );
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const l = (max + min) / 2;
  const d = max - min;
  if (d === 0) return [0, 0, l * 100];
  const s = d / (1 - Math.abs(2 * l - 1));
  const h =
    max === r
      ? ((g - b) / d) % 6
      : max === g
        ? (b - r) / d + 2
        : (r - g) / d + 4;
  return [h * 60, s * 100, l * 100];
}

/** Lightens toward white for positive amounts, darkens toward black for negative. */
function tone([h, s, l]: Hsl, amount: number): Hsl {
  return [h, s, amount >= 0 ? l + (100 - l) * amount : l * (1 + amount)];
}

function css([h, s, l]: Hsl): string {
  const clamp = (v: number) => Math.round(Math.max(0, Math.min(100, v)));
  return `hsl(${Math.round(((h % 360) + 360) % 360)} ${clamp(s)}% ${clamp(l)}%)`;
}

/** The studio's soft pastel take on a project color. */
function pastel([h, s, l]: Hsl): Hsl {
  return [h, s * 0.55, l + (82 - l) * 0.55];
}

const EYE = "#263331";
const GLINT = "rgba(255,255,244,0.75)";
const WINDOW = "#243e51";
const WINDOW_GLINT = "#87c6d4";
const WINDOW_SILL = "#acd9d6";
const TOP_LIGHT = "rgba(255,255,237,0.24)";
const LEFT_LIGHT = "rgba(255,255,237,0.13)";

/** A rect on the 16-unit grid. */
export type PixelRect = {
  x: number;
  y: number;
  w: number;
  h: number;
  fill: string;
};

export type PixelLayers = {
  body: PixelRect[];
  /** Eyes open; the blink swaps in `closed`. */
  open: PixelRect[];
  closed: PixelRect[];
};

const rect = (
  x: number,
  y: number,
  w: number,
  h: number,
  fill: string,
): PixelRect => ({
  x: INSET + x * CELL,
  y: INSET + y * CELL,
  w: w * CELL,
  h: h * CELL,
  fill,
});

const cache = new Map<string, PixelLayers>();

/**
 * Pixel rects for one mascot frame. `talking` swaps in the project mascot's
 * talk frame — mouths chew, legs shuffle — while its agent is mid-turn.
 */
export function pixelLayers(
  name: string,
  color: string,
  talking = false,
): PixelLayers {
  const key = `${name}|${color}|${talking}`;
  const hit = cache.get(key);
  if (hit) return hit;

  const known = SPRITES[name] ? name : "ghost";
  const rows = pixelSprite(known, talking);
  const fig = figure(known);
  const base = pastel(parse(color));
  const filled = (x: number, y: number) => rows[y]?.[x] === "#";
  const material = (x: number, y: number) => {
    const own = fig.material ? fig.material(x, y, base) : base;
    // Light falls from the top-left, a touch brighter toward that corner.
    return tone(own, 0.04 + (7 - y) * 0.009 + (3 - x) * 0.006);
  };

  const cells: Cell[] = [];
  for (let y = 0; y < SIZE; y++) {
    for (let x = 0; x < SIZE; x++) if (filled(x, y)) cells.push([x, y]);
  }

  const body: PixelRect[] = [];
  // Extruded edge: the silhouette again, nudged down-right in shade.
  const depth = 0.14;
  for (const [x, y] of cells) {
    body.push(
      rect(x + depth, y + depth * 0.66, 1, 1, css(tone(material(x, y), -0.32))),
    );
  }
  for (const [x, y] of cells) {
    body.push(rect(x, y, 1, 1, css(material(x, y))));
    if (!filled(x, y - 1)) body.push(rect(x, y, 1, 0.08, TOP_LIGHT));
    if (!filled(x - 1, y)) body.push(rect(x, y, 0.06, 1, LEFT_LIGHT));
  }
  for (const [x, y, w, h, fill] of (talking
    ? fig.details?.talk
    : fig.details?.rest) ?? []) {
    body.push(rect(x, y, w, h, fill));
  }

  const rocket = known === "rocket";
  const open: PixelRect[] = [];
  const closed: PixelRect[] = [];
  for (const [x, y] of pixelEyes(known, talking)) {
    open.push(rect(x, y, 1, 1, rocket ? WINDOW : EYE));
    open.push(
      rect(x + 0.15, y + 0.15, 0.25, 0.25, rocket ? WINDOW_GLINT : GLINT),
    );
    if (rocket) open.push(rect(x + 0.13, y + 0.69, 0.56, 0.11, WINDOW_SILL));
    // The lid takes the face's color, with the lash line along the bottom.
    closed.push(rect(x, y, 1, 1, css(material(x, y))));
    closed.push(rect(x, y + 0.55, 1, 0.25, rocket ? WINDOW : EYE));
  }

  const layers = { body, open, closed };
  cache.set(key, layers);
  return layers;
}

/** Light bulb shown over a mascot that needs you, in grid units. */
export const BULB: readonly PixelRect[] = [
  { x: 1, y: 0, w: 2, h: 1, fill: "#FFF3B0" },
  { x: 0, y: 1, w: 1, h: 1, fill: "#FFF3B0" },
  { x: 1, y: 1, w: 2, h: 1, fill: "#FFD24A" },
  { x: 3, y: 1, w: 1, h: 1, fill: "#FFD24A" },
  { x: 0, y: 2, w: 4, h: 1, fill: "#FFD24A" },
  { x: 1, y: 3, w: 2, h: 1, fill: "#F29A1F" },
  { x: 1, y: 4, w: 2, h: 1, fill: "#8C8680" },
];
