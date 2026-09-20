// @vitest-environment happy-dom
import { describe, expect, it } from "vitest";
import {
  atScrollEdge,
  hasScrollRoom,
  lockOverscroll,
} from "./useLockOverscroll";

const box = (over: Partial<Record<string, number>> = {}) => ({
  scrollTop: 0,
  scrollLeft: 0,
  clientHeight: 400,
  clientWidth: 400,
  scrollHeight: 800,
  scrollWidth: 400,
  ...over,
});

/** happy-dom reports 0 for every layout box, so the sizes are declared. */
function sized(el: HTMLElement, sizes: Record<string, number>) {
  for (const [name, value] of Object.entries(sizes)) {
    Object.defineProperty(el, name, { value, configurable: true });
  }
  return el;
}

describe("useLockOverscroll wheel guard", () => {
  it.each(["svg", "path"])(
    "allows scrolling a nested container through an SVG %s target",
    (tag) => {
      const outer = sized(
        document.createElement("div"),
        box({ scrollTop: 400 }),
      );
      const inner = sized(document.createElement("div"), box());
      inner.style.overflowY = "auto";
      const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
      const path = document.createElementNS(
        "http://www.w3.org/2000/svg",
        "path",
      );
      svg.append(path);
      inner.append(svg);
      outer.append(inner);
      document.body.append(outer);
      const detach = lockOverscroll(outer);
      const wheel = new WheelEvent("wheel", {
        deltaY: 40,
        bubbles: true,
        cancelable: true,
      });
      (tag === "svg" ? svg : path).dispatchEvent(wheel);
      expect(wheel.defaultPrevented).toBe(false);
      detach();
      outer.remove();
    },
  );

  it.each([
    {
      overflowX: "auto",
      overflowY: "hidden",
      deltaX: 0,
      deltaY: 40,
      blocked: true,
    },
    {
      overflowX: "hidden",
      overflowY: "auto",
      deltaX: 40,
      deltaY: 0,
      blocked: true,
    },
    {
      overflowX: "auto",
      overflowY: "hidden",
      deltaX: 0,
      deltaY: -40,
      blocked: true,
    },
    {
      overflowX: "hidden",
      overflowY: "auto",
      deltaX: -40,
      deltaY: 0,
      blocked: true,
    },
    {
      overflowX: "auto",
      overflowY: "hidden",
      deltaX: 40,
      deltaY: 40,
      blocked: false,
    },
    {
      overflowX: "hidden",
      overflowY: "auto",
      deltaX: 40,
      deltaY: 40,
      blocked: false,
    },
  ])(
    "matches nested overflow to the gesture's axis ($overflowX, $overflowY, $deltaX, $deltaY)",
    ({ overflowX, overflowY, deltaX, deltaY, blocked }) => {
      const outer = sized(
        document.createElement("div"),
        box({
          scrollTop: deltaY < 0 ? 0 : 400,
          scrollLeft: deltaX < 0 ? 0 : 400,
          scrollWidth: 800,
        }),
      );
      const inner = sized(
        document.createElement("div"),
        box({ scrollTop: 100, scrollLeft: 100, scrollWidth: 800 }),
      );
      Object.assign(inner.style, { overflowX, overflowY });
      outer.append(inner);
      document.body.append(outer);
      const detach = lockOverscroll(outer);
      const wheel = new WheelEvent("wheel", {
        deltaX,
        deltaY,
        bubbles: true,
        cancelable: true,
      });
      inner.dispatchEvent(wheel);
      expect(wheel.defaultPrevented).toBe(blocked);
      detach();
      outer.remove();
    },
  );

  it("does not block horizontal wheel on vertically scrolling containers", () => {
    expect(atScrollEdge(box(), { deltaX: 40, deltaY: 0 })).toBe(false);
    expect(atScrollEdge(box(), { deltaX: -40, deltaY: 0 })).toBe(false);
  });

  it("still blocks vertical rubber-band at the top and bottom", () => {
    expect(atScrollEdge(box(), { deltaX: 0, deltaY: -40 })).toBe(true);
    expect(
      atScrollEdge(box({ scrollTop: 400 }), { deltaX: 0, deltaY: 40 }),
    ).toBe(true);
  });

  it("reads room in the direction the wheel is pushing", () => {
    expect(hasScrollRoom(box(), { deltaX: 0, deltaY: 40 })).toBe(true);
    expect(hasScrollRoom(box(), { deltaX: 0, deltaY: -40 })).toBe(false);
    expect(
      hasScrollRoom(box({ scrollTop: 400 }), { deltaX: 0, deltaY: 40 }),
    ).toBe(false);
  });

  it("leaves the gesture alone when a scroller inside still has room", () => {
    // A pinned transcript with a part-scrolled field in it: cancelling the
    // wheel here would cancel the field's own scrolling too.
    const scroller = sized(document.createElement("div"), {
      scrollTop: 400,
      clientHeight: 400,
      scrollHeight: 800,
      clientWidth: 400,
      scrollWidth: 400,
    });
    const field = sized(document.createElement("textarea"), {
      scrollTop: 0,
      clientHeight: 160,
      scrollHeight: 600,
      clientWidth: 400,
      scrollWidth: 400,
    });
    field.style.overflowY = "auto";
    scroller.append(field);
    document.body.append(scroller);
    const detach = lockOverscroll(scroller);

    const down = new WheelEvent("wheel", {
      deltaY: 40,
      bubbles: true,
      cancelable: true,
    });
    field.dispatchEvent(down);
    expect(down.defaultPrevented).toBe(false);

    // Once the field bottoms out the lock takes over again.
    Object.defineProperty(field, "scrollTop", {
      value: 440,
      configurable: true,
    });
    const spent = new WheelEvent("wheel", {
      deltaY: 40,
      bubbles: true,
      cancelable: true,
    });
    field.dispatchEvent(spent);
    expect(spent.defaultPrevented).toBe(true);

    detach();
    scroller.remove();
  });
});
