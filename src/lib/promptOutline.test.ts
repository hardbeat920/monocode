import { describe, expect, it } from "vitest";
import { activePromptId } from "./promptOutline";

const viewport = { top: 100, bottom: 500 };

function anchor(id: string, top: number, bottom: number) {
  return { id, top, bottom };
}

describe("activePromptId", () => {
  it("returns null with no anchors", () => {
    expect(activePromptId(viewport, [])).toBeNull();
  });

  it("picks the topmost prompt inside the viewport", () => {
    const anchors = [
      anchor("a", 0, 40),
      anchor("b", 150, 190),
      anchor("c", 300, 340),
      anchor("d", 600, 640),
    ];
    expect(activePromptId(viewport, anchors)).toBe("b");
  });

  it("counts a prompt cut by the viewport top as inside", () => {
    const anchors = [anchor("a", 80, 120), anchor("b", 200, 240)];
    expect(activePromptId(viewport, anchors)).toBe("a");
  });

  it("lets a prompt that peeks in at the bottom win over the reply above", () => {
    const anchors = [anchor("a", 0, 40), anchor("b", 480, 520)];
    expect(activePromptId(viewport, anchors)).toBe("b");
  });

  it("falls back to the last prompt above the viewport", () => {
    const anchors = [
      anchor("a", 0, 20),
      anchor("b", 40, 60),
      anchor("c", 600, 640),
    ];
    expect(activePromptId(viewport, anchors)).toBe("b");
  });

  it("treats a prompt that ends at the viewport top as above", () => {
    const anchors = [anchor("a", 60, 100), anchor("b", 700, 740)];
    expect(activePromptId(viewport, anchors)).toBe("a");
  });

  it("falls back to the first prompt when all sit below", () => {
    const anchors = [anchor("a", 600, 640), anchor("b", 700, 740)];
    expect(activePromptId(viewport, anchors)).toBe("a");
  });

  it("marks the last prompt at the end of the transcript", () => {
    const anchors = [
      anchor("a", 120, 160),
      anchor("b", 260, 300),
      anchor("c", 400, 440),
    ];
    expect(activePromptId(viewport, anchors, 0)).toBe("c");
    expect(activePromptId(viewport, anchors, 16)).toBe("c");
  });

  it("keeps the topmost visible prompt while there is room to scroll", () => {
    const anchors = [anchor("a", 120, 160), anchor("b", 400, 440)];
    expect(activePromptId(viewport, anchors, 17)).toBe("a");
  });
});
