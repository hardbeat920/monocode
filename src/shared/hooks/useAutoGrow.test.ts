import { describe, expect, it } from "vitest";
import { fitTextarea } from "./useAutoGrow";

function field(contentHeight: number, shown = true) {
  const style = { height: "48px", overflowY: "" };
  return {
    style,
    // Like the DOM: the text's height, or the box's when that is taller.
    get scrollHeight() {
      return Math.max(contentHeight, Number.parseFloat(style.height));
    },
    getClientRects: () => ({ length: shown ? 1 : 0 }),
  };
}

describe("fitTextarea", () => {
  it("shrinks to the text and does not scroll while it fits", () => {
    const el = field(28);
    fitTextarea(el, 160);
    expect(el.style).toEqual({ height: "28px", overflowY: "hidden" });
  });

  it("stops at the max height and scrolls from there", () => {
    const el = field(400);
    fitTextarea(el, 160);
    expect(el.style).toEqual({ height: "160px", overflowY: "auto" });
  });

  it("leaves a hidden field alone until it can be measured", () => {
    const el = field(28, false);
    fitTextarea(el, 160);
    expect(el.style).toEqual({ height: "48px", overflowY: "" });
  });
});
