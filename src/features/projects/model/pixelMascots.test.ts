import { expect, it } from "vitest";
import { PROJECT_MASCOTS } from "./projectMascots";
import {
  PIXEL_GRID,
  PIXEL_MASCOT_NAMES,
  pixelEyes,
  pixelLayers,
  pixelSprite,
} from "./pixelMascots";

it("has a pixel twin for every project mascot", () => {
  const ours = [...PIXEL_MASCOT_NAMES].sort();
  const projects = PROJECT_MASCOTS.map((mascot) => mascot.name).sort();
  expect(ours).toEqual(projects);
});

it("puts every eye in a hole of its frame", () => {
  for (const name of PIXEL_MASCOT_NAMES) {
    for (const talking of [false, true]) {
      const rows = pixelSprite(name, talking);
      expect(rows).toHaveLength(PIXEL_GRID / 2);
      for (const [x, y] of pixelEyes(name, talking)) {
        expect(rows[y][x]).toBe(".");
      }
    }
  }
});

it("falls back to the ghost for unknown names", () => {
  expect(pixelSprite("unicorn")).toBe(pixelSprite("ghost"));
  expect(pixelLayers("unicorn", "#ff8800")).toEqual(
    pixelLayers("ghost", "#ff8800"),
  );
});

it("gives every mascot open and closed eyes", () => {
  for (const name of PIXEL_MASCOT_NAMES) {
    const layers = pixelLayers(name, "hsl(211 92% 62%)");
    expect(layers.open.length).toBeGreaterThan(0);
    expect(layers.closed.length).toBeGreaterThan(0);
  }
});

it("softens the project color into a pastel body", () => {
  const [face] = pixelLayers("invader", "not a color").body.filter(
    (rect) => rect.x === 5 && rect.y === 3.5,
  );
  expect(face.fill).toMatch(/^hsl\(280 \d+% \d+%\)$/);
});
