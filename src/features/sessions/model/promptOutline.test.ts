import { describe, expect, it } from "vitest";
import {
  activePromptId,
  barLift,
  previewLines,
  promptPreview,
} from "./promptOutline";
import type { Block } from "./session";

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

describe("barLift", () => {
  it("is flat with no hovered bar", () => {
    expect(barLift(3, null)).toBe(0);
    expect(barLift(3, -1)).toBe(0);
  });

  it("peaks on the hovered bar and tapers over the ripple span", () => {
    expect(barLift(3, 3)).toBe(1);
    expect(barLift(2, 3)).toBeCloseTo(2 / 3);
    expect(barLift(5, 3)).toBeCloseTo(1 / 3);
    expect(barLift(6, 3)).toBe(0);
  });
});

describe("previewLines", () => {
  it("strips markers and collapses blank lines", () => {
    const text = "# Heading\n\n- **first** point\n\n2) second point\n";
    expect(previewLines(text, 2)).toEqual(["Heading", "first point"]);
  });

  it("skips fenced code and rules", () => {
    const text = "---\n```ts\nconst a = 1;\n```\nAfter the code.";
    expect(previewLines(text, 2)).toEqual(["After the code."]);
  });

  it("stops at the line budget", () => {
    expect(previewLines("a\nb\nc", 2)).toEqual(["a", "b"]);
  });
});

describe("promptPreview", () => {
  const block = (id: string, role: Block["role"], text: string): Block => ({
    id,
    role,
    text,
  });

  it("pairs a prompt with the head of its reply", () => {
    const blocks = [
      block("u1", "user", "First ask"),
      block("t1", "tool", "ran something"),
      block("a1", "assistant", "Yes.\nBuild the control plane."),
      block("u2", "user", "Second ask"),
      block("a2", "assistant", "Later reply"),
    ];
    expect(promptPreview(blocks, "u1")).toEqual({
      title: "First ask",
      reply: "Yes.",
      detail: "Build the control plane.",
    });
  });

  it("leaves the reply out when the turn has none yet", () => {
    const blocks = [block("u1", "user", "Only ask")];
    expect(promptPreview(blocks, "u1")).toEqual({
      title: "Only ask",
      reply: undefined,
      detail: undefined,
    });
  });

  it("returns null for an unknown prompt", () => {
    expect(promptPreview([], "missing")).toBeNull();
  });
});
