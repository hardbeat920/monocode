import { describe, expect, it } from "vitest";
import { COMPOSER_MAX_HEIGHT, resizeComposer } from "./composerResize";

function field(scrollHeight: number, height = "") {
  return { style: { height }, scrollHeight };
}

describe("resizeComposer", () => {
  it("grows the field to fit the draft", () => {
    const el = field(88);
    resizeComposer(el);
    expect(el.style.height).toBe("88px");
  });

  it("stops growing at the max height", () => {
    const el = field(400);
    resizeComposer(el);
    expect(el.style.height).toBe(`${COMPOSER_MAX_HEIGHT}px`);
  });

  it("scrolls only once the draft passes the max height", () => {
    const fits = field(88);
    resizeComposer(fits);
    expect(fits.style).toMatchObject({ overflowY: "hidden" });
    const overflows = field(400);
    resizeComposer(overflows);
    expect(overflows.style).toMatchObject({ overflowY: "auto" });
  });

  it("supports a taller field without changing the composer default", () => {
    const el = field(400);
    resizeComposer(el, Number.POSITIVE_INFINITY);
    expect(el.style.height).toBe("400px");
  });

  it.each([
    [88, 140],
    [140, 88],
    [160, 400],
  ])(
    "keeps the composer space while measuring from %i to %i",
    (height, nextHeight) => {
      const wrapper = { style: { minHeight: "20px" }, offsetHeight: height };
      const style = { height: `${height}px` };
      let measured = false;
      const el = {
        style,
        parentElement: wrapper,
        get scrollHeight() {
          if (style.height === "auto") {
            // Reading layout here must not briefly expand the transcript's
            // viewport and clamp its scroll position before the final resize.
            expect(wrapper.style.minHeight).toBe(`${height}px`);
            measured = true;
          }
          return nextHeight;
        },
      };
      resizeComposer(el);
      expect(measured).toBe(true);
      expect(style.height).toBe(
        `${Math.min(nextHeight, COMPOSER_MAX_HEIGHT)}px`,
      );
      expect(wrapper.style.minHeight).toBe("20px");
    },
  );

  it("leaves the height alone when the field has no layout box", () => {
    const wrapper = { style: { minHeight: "20px" }, offsetHeight: 0 };
    const el = { ...field(0, "88px"), parentElement: wrapper };
    resizeComposer(el);
    expect(el.style.height).toBe("88px");
    expect(wrapper.style.minHeight).toBe("20px");
  });
});
