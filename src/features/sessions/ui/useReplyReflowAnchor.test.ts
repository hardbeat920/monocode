// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { beginLayoutTransition } from "../../../shared/lib/layoutTransition";
import { useReplyReflowAnchor } from "./useReplyReflowAnchor";

it("keeps the reader's character through scrolling, reflow, and cached tab return", () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const frames: FrameRequestCallback[] = [];
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
    frames.push(callback);
    return frames.length;
  });
  vi.stubGlobal("cancelAnimationFrame", vi.fn());
  let resize!: ResizeObserverCallback;
  vi.stubGlobal(
    "ResizeObserver",
    class {
      constructor(callback: ResizeObserverCallback) {
        resize = callback;
      }
      observe() {}
      disconnect() {}
    },
  );
  const host = document.createElement("div");
  const scroller = document.createElement("div");
  scroller.innerHTML =
    '<div><div data-chat-message-role="assistant">A visible reply</div></div>';
  document.body.append(host, scroller);
  const text = scroller.querySelector("[data-chat-message-role]")!.firstChild!;
  const whitespace = document.createTextNode(" ");
  text.parentNode!.insertBefore(whitespace, text);
  let width = 200;
  let glyphTop = 48;
  let merged = false;
  let caretOffset = 3;
  let now = 0;
  vi.spyOn(performance, "now").mockImplementation(() => now);
  vi.spyOn(Range.prototype, "getBoundingClientRect").mockImplementation(
    function (this: Range) {
      if (this.startContainer === whitespace && width === 100)
        return { top: 0, width: 0, height: 0 } as DOMRect;
      return {
        width: 8,
        height: 16,
        top:
          glyphTop -
          scroller.scrollTop +
          (merged && this.startOffset === 3 ? 250 : 0),
      } as DOMRect;
    },
  );
  Object.defineProperty(scroller, "clientWidth", { get: () => width });
  scroller.getBoundingClientRect = () =>
    ({ top: 0, left: 0, width, height: 200 }) as DOMRect;
  const descriptor = Object.getOwnPropertyDescriptor(
    document,
    "caretRangeFromPoint",
  );
  Object.defineProperty(document, "caretRangeFromPoint", {
    configurable: true,
    value: (_x: number, y: number) => {
      const range = document.createRange();
      if (y === 32) range.selectNodeContents(scroller);
      else if (y === 48 && _x === width * 0.25) {
        range.setStart(whitespace, 0);
        range.setEnd(whitespace, 1);
      } else {
        range.setStart(text, caretOffset);
        range.setEnd(text, caretOffset + 1);
      }
      return range;
    },
  });
  const adjustments = { current: undefined as number | undefined };
  const remember = vi.fn((el: HTMLElement) => {
    adjustments.current = el.scrollTop;
  });
  const following = { current: false };
  function Probe({ visible = true }: { visible?: boolean }) {
    useReplyReflowAnchor(scroller, visible, following, remember, adjustments);
    return null;
  }
  const root = createRoot(host);
  try {
    act(() => root.render(createElement(Probe)));
    act(() => frames.shift()!(0));
    width = 100;
    glyphTop = 148;
    act(() => resize([], {} as ResizeObserver));
    expect(scroller.scrollTop).toBe(100);
    expect(remember).toHaveBeenCalledWith(scroller);

    // A queued correction from another layout observer is not new user
    // input and must not replace the character retained across a divider drag.
    scroller.scrollTop = 130;
    adjustments.current = scroller.scrollTop;
    scroller.dispatchEvent(new Event("scroll"));
    act(() => {
      while (frames.length) frames.shift()!(now);
    });
    act(() => resize([], {} as ResizeObserver));
    expect(scroller.scrollTop).toBe(100);

    // A closing pane briefly applies the final layout before its slide starts.
    // The resulting native clamp must not replace the bookmarked character.
    const endTransition = beginLayoutTransition();
    try {
      scroller.scrollTop = 50;
      scroller.dispatchEvent(new Event("scroll"));
      act(() => {
        while (frames.length) frames.shift()!(now);
      });
      width = 200;
      glyphTop = 248;
      act(() => resize([], {} as ResizeObserver));
      expect(scroller.scrollTop).toBe(200);
    } finally {
      endTransition();
    }
    width = 100;
    glyphTop = 148;
    act(() => resize([], {} as ResizeObserver));
    expect(scroller.scrollTop).toBe(100);

    // Momentum continues after the original wheel event's quiet period.
    // Each scroll must update the reader's glyph until the gesture settles.
    scroller.dispatchEvent(new WheelEvent("wheel", { deltaY: 40 }));
    for (const [time, movement] of [
      [0, 20],
      [150, 30],
      [250, 40],
    ]) {
      now = time;
      scroller.scrollTop = 100 + movement;
      scroller.dispatchEvent(new Event("scroll"));
      act(() => {
        while (frames.length) frames.shift()!(now);
      });
    }
    width = 200;
    glyphTop = 248;
    act(() => resize([], {} as ResizeObserver));
    expect(scroller.scrollTop).toBe(240);

    // A native scroll can reach the main thread after the input's quiet
    // window, even when the wheel's animation frame has already run.
    now = 1000;
    scroller.dispatchEvent(new WheelEvent("wheel", { deltaY: 40 }));
    act(() => {
      while (frames.length) frames.shift()!(now);
    });
    // The motion hook hands its visual offset to native scrolling first.
    // That programmatic event must not consume the pending wheel input.
    scroller.scrollTop = 250;
    adjustments.current = scroller.scrollTop;
    scroller.dispatchEvent(new Event("scroll"));
    act(() => {
      while (frames.length) frames.shift()!(now);
    });
    now = 1250;
    scroller.scrollTop = 280;
    scroller.dispatchEvent(new Event("scroll"));
    act(() => {
      while (frames.length) frames.shift()!(now);
    });
    width = 100;
    glyphTop = 348;
    act(() => resize([], {} as ResizeObserver));
    expect(scroller.scrollTop).toBe(380);

    // Streamed layout can change just after scrolling stops. Recent input
    // alone must not discard the bookmark while the offset is stationary.
    now = 2000;
    scroller.dispatchEvent(new WheelEvent("wheel", { deltaY: -40 }));
    act(() => {
      while (frames.length) frames.shift()!(now);
    });
    glyphTop = 388;
    act(() => resize([], {} as ResizeObserver));
    expect(scroller.scrollTop).toBe(420);

    // Reattaching a cached transcript restores a native offset before the
    // resize observer restores the reader's glyph. Ignore that queued event.
    merged = false;
    caretOffset = 11;
    act(() => root.render(createElement(Probe, { visible: false })));
    scroller.scrollTop = 460;
    act(() => root.render(createElement(Probe)));
    scroller.dispatchEvent(new Event("scroll"));
    act(() => {
      while (frames.length) frames.shift()!(now);
    });
    act(() => resize([], {} as ResizeObserver));
    expect(scroller.scrollTop).toBe(420);

    // Word-fade cleanup can merge text into a surviving node. That node's old
    // offset now names a different glyph even though it is still connected.
    caretOffset = 3;
    const prefix = document.createTextNode("Earlier ");
    text.parentNode!.insertBefore(prefix, text);
    now = 3000;
    scroller.dispatchEvent(new WheelEvent("wheel", { deltaY: -40 }));
    act(() => {
      while (frames.length) frames.shift()!(now);
    });
    prefix.remove();
    text.textContent = "Earlier A visible reply";
    merged = true;
    now = 3250;
    act(() => resize([], {} as ResizeObserver));
    expect(scroller.scrollTop).toBe(420);
  } finally {
    act(() => root.unmount());
    host.remove();
    scroller.remove();
    if (descriptor)
      Object.defineProperty(document, "caretRangeFromPoint", descriptor);
    else Reflect.deleteProperty(document, "caretRangeFromPoint");
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  }
});
