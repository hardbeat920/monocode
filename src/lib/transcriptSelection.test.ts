import { describe, expect, it } from "vitest";
import {
  placeSelectionMenu,
  placeFlyoutMenu,
  validateTranscriptSelection,
} from "./transcriptSelection";

describe("validateTranscriptSelection", () => {
  it("accepts non-empty text within one settled response", () => {
    expect(
      validateTranscriptSelection({
        text: " selected ",
        collapsed: false,
        anchorResponseId: "response-1",
        focusResponseId: "response-1",
      }),
    ).toBe("selected");
  });

  it.each([
    [true, "response-1", "response-1"],
    [false, null, "response-1"],
    [false, "response-1", null],
    [false, "response-1", "response-2"],
  ])("rejects an invalid selection %#", (collapsed, anchor, focus) => {
    expect(
      validateTranscriptSelection({
        text: "selected",
        collapsed,
        anchorResponseId: anchor,
        focusResponseId: focus,
      }),
    ).toBeNull();
  });
});

describe("placeSelectionMenu", () => {
  const menu = { width: 120, height: 32 };

  it("prefers above the selection using the measured menu size", () => {
    expect(
      placeSelectionMenu(
        { left: 100, top: 100, bottom: 120, width: 100 },
        menu,
        { width: 800, height: 600 },
      ),
    ).toEqual({ side: "top", top: 62, left: 90 });
  });

  it("flips below near the top and clamps horizontally", () => {
    expect(
      placeSelectionMenu({ left: -50, top: 10, bottom: 30, width: 70 }, menu, {
        width: 320,
        height: 600,
      }),
    ).toEqual({ side: "bottom", top: 36, left: 8 });
  });

  it("keeps an oversized menu inside the leading viewport edge", () => {
    expect(
      placeSelectionMenu(
        { left: 100, top: 100, bottom: 120, width: 100 },
        { width: 400, height: 700 },
        { width: 320, height: 600 },
      ).left,
    ).toBe(8);
  });
});

describe("placeFlyoutMenu", () => {
  const menu = { width: 200, height: 160 };

  it("opens to the right of the parent and overlaps the seam", () => {
    expect(
      placeFlyoutMenu({ left: 40, width: 220 }, 80, menu, {
        width: 800,
        height: 600,
      }),
    ).toEqual({ side: "right", left: 256, top: 80 });
  });

  it("flips left when the right side would overflow", () => {
    expect(
      placeFlyoutMenu({ left: 500, width: 220 }, 80, menu, {
        width: 740,
        height: 600,
      }),
    ).toEqual({ side: "left", left: 304, top: 80 });
  });

  it("clamps vertically so a tall flyout stays on screen", () => {
    expect(
      placeFlyoutMenu({ left: 40, width: 220 }, 500, menu, {
        width: 800,
        height: 600,
      }).top,
    ).toBe(432);
  });
});
