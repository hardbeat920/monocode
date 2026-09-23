import { describe, expect, it } from "vitest";
import {
  MAX_CANVAS_AREA,
  MAX_CANVAS_SIDE,
  renderPixelRatio,
} from "./pdfDocument";

describe("renderPixelRatio", () => {
  it("keeps the display ratio when the bitmap fits", () => {
    expect(renderPixelRatio(612, 792, 2)).toBe(2);
  });

  it("lowers the ratio to keep a zoomed page within the area limit", () => {
    // A letter page at 1600% on a 2x display would need about 2 GB.
    const width = 612 * 16;
    const height = 792 * 16;
    const ratio = renderPixelRatio(width, height, 2);

    expect(ratio).toBeLessThan(2);
    expect(width * ratio * height * ratio).toBeLessThanOrEqual(
      MAX_CANVAS_AREA * 1.0001,
    );
  });

  it("lowers the ratio to keep a long, narrow page within the side limit", () => {
    const ratio = renderPixelRatio(200, 9000, 2);

    expect(9000 * ratio).toBeLessThanOrEqual(MAX_CANVAS_SIDE);
  });
});
