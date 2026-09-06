import { describe, expect, it } from "vitest";
import {
  normalizeUiScale,
  UI_SCALE_DEFAULT,
  UI_SCALE_MAX,
  UI_SCALE_MIN,
  uiScaleCommand,
  zoomInUiScale,
  zoomOutUiScale,
} from "./uiScale";

describe("ui scale", () => {
  it("clamps to the supported range and rounds to one decimal", () => {
    expect(normalizeUiScale(1)).toBe(1);
    expect(normalizeUiScale(1.05)).toBe(1.1);
    expect(normalizeUiScale(0)).toBe(UI_SCALE_MIN);
    expect(normalizeUiScale(99)).toBe(UI_SCALE_MAX);
    expect(normalizeUiScale(Number.NaN)).toBe(UI_SCALE_DEFAULT);
    expect(normalizeUiScale("junk")).toBe(UI_SCALE_DEFAULT);
  });

  it("steps in and out without float drift", () => {
    expect(zoomInUiScale(1)).toBe(1.1);
    expect(zoomOutUiScale(1.1)).toBe(1);
    // 10 steps up from 1 lands exactly on 2, not 1.9999999
    let scale = 1;
    for (let i = 0; i < 10; i += 1) scale = zoomInUiScale(scale);
    expect(scale).toBe(UI_SCALE_MAX);
    expect(zoomInUiScale(UI_SCALE_MAX)).toBe(UI_SCALE_MAX);
    expect(zoomOutUiScale(UI_SCALE_MIN)).toBe(UI_SCALE_MIN);
  });

  it("maps browser-standard zoom keys", () => {
    expect(uiScaleCommand({ key: "+", code: "Equal" })).toBe("zoom-in");
    expect(uiScaleCommand({ key: "=", code: "Equal" })).toBe("zoom-in");
    expect(uiScaleCommand({ key: "Add", code: "NumpadAdd" })).toBe("zoom-in");
    expect(uiScaleCommand({ key: "-", code: "Minus" })).toBe("zoom-out");
    expect(uiScaleCommand({ key: "_", code: "Minus" })).toBe("zoom-out");
    expect(
      uiScaleCommand({ key: "Subtract", code: "NumpadSubtract" }),
    ).toBe("zoom-out");
    expect(uiScaleCommand({ key: "0", code: "Digit0" })).toBe("zoom-reset");
    expect(uiScaleCommand({ key: "0", code: "Numpad0" })).toBe("zoom-reset");
    expect(uiScaleCommand({ key: "p", code: "KeyP" })).toBeNull();
  });
});
