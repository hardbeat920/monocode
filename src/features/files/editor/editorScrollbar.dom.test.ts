// @vitest-environment happy-dom
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { afterEach, describe, expect, it } from "vitest";
import { applyUiScale } from "../../settings/model/uiScale";
import { editorScrollbar } from "./editorScrollbar";

const NON_DEFAULT_SCALES = [0.5, 1.5, 2];
const TRACK_TOP = 40;

type Geometry = {
  scrollTop?: number;
  scrollHeight: number;
  clientHeight: number;
  trackHeight: number;
};

function stub(element: HTMLElement, key: string, value: number) {
  Object.defineProperty(element, key, {
    configurable: true,
    writable: true,
    value,
  });
}

// happy-dom has no layout. These CSS-pixel values are what a zoomed webview
// reports to the scrollbar, because page zoom never changes CSS pixels.
function mountEditor({
  scrollTop = 0,
  scrollHeight,
  clientHeight,
  trackHeight,
}: Geometry) {
  const parent = document.createElement("div");
  document.body.appendChild(parent);
  const view = new EditorView({
    state: EditorState.create({ doc: "x", extensions: [editorScrollbar] }),
    parent,
  });
  const rail = view.dom.querySelector<HTMLElement>(".cm-editorScrollbar")!;
  const thumb = rail.querySelector<HTMLElement>(".cm-editorScrollbarThumb")!;
  stub(view.scrollDOM, "scrollTop", scrollTop);
  stub(view.scrollDOM, "scrollHeight", scrollHeight);
  stub(view.scrollDOM, "clientHeight", clientHeight);
  stub(rail, "clientHeight", trackHeight);
  rail.getBoundingClientRect = () => new DOMRect(0, TRACK_TOP, 18, trackHeight);
  return { view, rail, thumb };
}

function nextFrame() {
  return new Promise((resolve) => requestAnimationFrame(resolve));
}

function thumbTop(thumb: HTMLElement) {
  const match = /translate3d\(0, ([\d.]+)px, 0\)/.exec(thumb.style.transform);
  return Number(match?.[1]);
}

function pointerDown(target: HTMLElement, clientY: number) {
  target.dispatchEvent(
    new PointerEvent("pointerdown", {
      bubbles: true,
      button: 0,
      pointerId: 1,
      clientY,
    }),
  );
}

afterEach(async () => {
  document.body.replaceChildren();
  await applyUiScale(1);
});

describe.each(NON_DEFAULT_SCALES)("editor scrollbar at %sx scale", (scale) => {
  it("keeps CSS-pixel rail dimensions so the webview zoom scales them once", async () => {
    await applyUiScale(scale);
    const { view, rail, thumb } = mountEditor({
      scrollHeight: 1_000,
      clientHeight: 100,
      trackHeight: 200,
    });

    expect(getComputedStyle(rail).width).toBe("18px");
    expect(getComputedStyle(view.contentDOM).paddingRight).toBe("18px");
    expect(getComputedStyle(thumb).left).toBe("3px");
    expect(getComputedStyle(thumb).right).toBe("3px");
    view.destroy();
  });

  it("keeps a minimum-height thumb inside a short rail", async () => {
    await applyUiScale(scale);
    const { view, thumb } = mountEditor({
      scrollTop: 900,
      scrollHeight: 1_000,
      clientHeight: 100,
      trackHeight: 20,
    });

    await nextFrame();

    // scrollbarMetrics owns the minimum; a CSS min-height would overflow it.
    expect(getComputedStyle(thumb).minHeight).toBe("");
    expect(thumb.style.height).toBe("20px");
    expect(thumbTop(thumb)).toBe(0);
    view.destroy();
  });

  it("centers the thumb on a track click", async () => {
    await applyUiScale(scale);
    const { view, rail, thumb } = mountEditor({
      scrollHeight: 1_000,
      clientHeight: 100,
      trackHeight: 200,
    });

    pointerDown(rail, TRACK_TOP + 100);
    await nextFrame();

    // Thumb is the 32px minimum, so its middle lands on the click: 100 - 16.
    expect(thumb.style.height).toBe("32px");
    expect(view.scrollDOM.scrollTop).toBeCloseTo((84 / 168) * 900);
    view.destroy();
  });
});
