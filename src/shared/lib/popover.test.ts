import { describe, expect, it } from "vitest";
import { placePopover, type AnchorRect } from "./popover";

function anchor(
  left: number,
  top: number,
  width: number,
  height: number,
): AnchorRect {
  return { left, top, width, height, right: left + width, bottom: top + height };
}

const menu = { width: 200, height: 150 };
const viewport = { width: 800, height: 600 };

describe("placePopover", () => {
  it("opens below the anchor, aligned to its leading edge", () => {
    expect(placePopover(anchor(100, 100, 80, 20), menu, viewport)).toEqual({
      side: "bottom",
      left: 100,
      top: 126,
      width: 200,
      maxHeight: 466,
    });
  });

  it("flips above when the space below cannot hold it", () => {
    expect(placePopover(anchor(100, 560, 80, 20), menu, viewport)).toEqual({
      side: "top",
      left: 100,
      bottom: 46,
      width: 200,
      maxHeight: 546,
    });
  });

  it("pins a top-side popover by its bottom edge so growth stays put", () => {
    const position = placePopover(anchor(100, 400, 80, 20), menu, viewport, {
      side: "top",
    });
    expect(position.bottom).toBe(206);
    expect(position.top).toBeUndefined();
  });

  it("keeps a menu inside the trailing viewport edge", () => {
    expect(
      placePopover(anchor(700, 100, 80, 20), menu, viewport).left,
    ).toBe(592);
  });

  it("aligns to the anchor's center and trailing edge", () => {
    expect(
      placePopover(anchor(300, 100, 80, 20), menu, viewport, {
        align: "center",
      }).left,
    ).toBe(240);
    expect(
      placePopover(anchor(400, 100, 80, 20), menu, viewport, { align: "end" })
        .left,
    ).toBe(280);
  });

  it("caps the height by the requested maximum", () => {
    expect(
      placePopover(anchor(100, 100, 80, 20), menu, viewport, {
        maxHeight: 200,
      }).maxHeight,
    ).toBe(200);
  });

  it("keeps the minimum height when neither side has room", () => {
    expect(
      placePopover(
        anchor(100, 100, 80, 20),
        menu,
        { width: 800, height: 200 },
        { side: "top", minHeight: 180 },
      ),
    ).toEqual({
      side: "top",
      left: 100,
      bottom: 106,
      width: 200,
      maxHeight: 180,
    });
  });

  it("narrows a popover wider than the viewport", () => {
    expect(
      placePopover(anchor(0, 100, 80, 20), menu, { width: 180, height: 600 }),
    ).toMatchObject({ left: 8, width: 164 });
  });

  it("hangs a flyout off the anchor's right edge, overlapping by the gap", () => {
    expect(
      placePopover(
        anchor(100, 100, 200, 24),
        { width: 240, height: 300 },
        viewport,
        { side: "right", gap: -4 },
      ),
    ).toEqual({
      side: "right",
      left: 296,
      top: 100,
      width: 240,
      maxHeight: 492,
    });
  });

  it("flips a flyout to the left when the right runs out", () => {
    expect(
      placePopover(
        anchor(200, 100, 200, 24),
        { width: 240, height: 300 },
        { width: 420, height: 600 },
        { side: "right", gap: -4 },
      ),
    ).toMatchObject({ side: "left", left: 8 });
  });

  it("clamps a flyout that would hang below the viewport", () => {
    expect(
      placePopover(
        anchor(100, 560, 200, 24),
        { width: 240, height: 300 },
        viewport,
        { side: "right" },
      ),
    ).toMatchObject({ top: 292, maxHeight: 300 });
  });
});
