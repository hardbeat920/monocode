// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  sessionComposerDropFromPoint,
  sessionDropChoice,
} from "./sessionComposerDrop";

afterEach(() => {
  document.body.innerHTML = "";
  vi.restoreAllMocks();
});

function composer(id: string) {
  const zone = document.createElement("div");
  zone.dataset.sessionContextDrop = id;
  const inner = document.createElement("textarea");
  zone.append(inner);
  document.body.append(zone);
  vi.spyOn(zone, "getBoundingClientRect").mockReturnValue({
    left: 100,
    width: 200,
    top: 0,
    height: 50,
  } as DOMRect);
  document.elementFromPoint = vi.fn(() => inner);
  return zone;
}

describe("session composer drops", () => {
  it("splits the composer into add-to-context and link halves", () => {
    expect(sessionDropChoice(150, { left: 100, width: 200 })).toBe("context");
    expect(sessionDropChoice(250, { left: 100, width: 200 })).toBe("link");
  });

  it("finds another session's composer under the pointer", () => {
    composer("target");
    expect(sessionComposerDropFromPoint(120, 10, "dragged")).toEqual({
      targetId: "target",
      choice: "context",
    });
    expect(sessionComposerDropFromPoint(280, 10, "dragged")?.choice).toBe("link");
  });

  it("ignores a session dropped on its own composer", () => {
    composer("same");
    expect(sessionComposerDropFromPoint(120, 10, "same")).toBeNull();
  });
});
