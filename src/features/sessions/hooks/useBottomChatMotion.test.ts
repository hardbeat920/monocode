// @vitest-environment happy-dom
import { act, createElement, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Block } from "../model/session";
import { useBottomChatMotion } from "./useBottomChatMotion";

let container: HTMLDivElement;
let root: Root;
let following: { current: boolean };
let positions: Map<string, number>;
let observers: Array<{ targets: Element[]; resize: () => void }>;
let frames: Map<number, FrameRequestCallback>;
let now: number;
let nextFrame: number;
let reduced: boolean;
let scrollHeight: number;
let top: number;
let scrollWrites: number[];
let entrances: Array<{
  message: HTMLElement;
  cancel: ReturnType<typeof vi.fn>;
}>;
let originalAnimate: PropertyDescriptor | undefined;

function Harness({
  blocks,
  enabled = true,
  busy = true,
  historicalBlockIds,
}: {
  blocks: Block[];
  enabled?: boolean;
  busy?: boolean;
  historicalBlockIds?: ReadonlySet<string>;
}) {
  const [scroller, setScroller] = useState<HTMLDivElement | null>(null);
  useBottomChatMotion(
    scroller,
    enabled,
    following,
    blocks,
    false,
    busy,
    historicalBlockIds,
  );
  return createElement(
    "div",
    { ref: setScroller, "data-scroller": true },
    createElement(
      "div",
      { "data-transcript-content": true },
      blocks.map((block) =>
        createElement(
          "div",
          {
            key: block.id,
            ref: (element: HTMLDivElement | null) => {
              if (!element) return;
              element.getBoundingClientRect = () => {
                const content = element.parentElement!;
                const offset = Number.parseFloat(
                  content.style.transform.match(/translateY\((.*)px\)/)?.[1] ??
                    "0",
                );
                return {
                  top: (positions.get(block.id) ?? 0) - top + offset,
                } as DOMRect;
              };
            },
          },
          createElement("div", {
            "data-chat-message": block.id,
            "data-chat-message-role": block.role,
          }),
        ),
      ),
    ),
  );
}

function render(
  blocks: Block[],
  enabled = true,
  busy = true,
  historicalBlockIds?: ReadonlySet<string>,
) {
  act(() =>
    root.render(
      createElement(Harness, { blocks, enabled, busy, historicalBlockIds }),
    ),
  );
}

function viewport() {
  const scroller = container.querySelector<HTMLDivElement>("[data-scroller]")!;
  const content = scroller.firstElementChild as HTMLElement;
  Object.defineProperties(scroller, {
    clientWidth: { get: () => 640 },
    clientHeight: { get: () => 400 },
    scrollHeight: { get: () => scrollHeight },
    scrollTop: {
      get: () => top,
      set: (value: number) => {
        scrollWrites.push(value);
        top = Math.max(0, Math.min(value, scrollHeight - 400));
      },
    },
  });
  const observer = observers.find((item) => item.targets.includes(content))!;
  act(() => observer.resize());
  return { scroller, content, resize: () => act(() => observer.resize()) };
}

function advance(ms: number) {
  now += ms;
  const queued = [...frames.values()];
  frames.clear();
  act(() => {
    for (const frame of queued) frame(now);
  });
}

function offset(content: HTMLElement) {
  return Number.parseFloat(
    content.style.transform.match(/translateY\((.*)px\)/)?.[1] ?? "0",
  );
}

beforeEach(() => {
  following = { current: true };
  positions = new Map();
  observers = [];
  frames = new Map();
  entrances = [];
  now = 0;
  nextFrame = 0;
  reduced = false;
  scrollHeight = 400;
  top = 0;
  scrollWrites = [];
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
    frames.set(++nextFrame, callback);
    return nextFrame;
  });
  vi.stubGlobal("cancelAnimationFrame", (id: number) => frames.delete(id));
  vi.stubGlobal("matchMedia", () => ({ matches: reduced }));
  vi.spyOn(performance, "now").mockImplementation(() => now);
  vi.stubGlobal(
    "ResizeObserver",
    class {
      targets: Element[] = [];
      constructor(readonly resize: () => void) {
        observers.push(this);
      }
      observe(target: Element) {
        this.targets.push(target);
      }
      disconnect() {
        this.targets = [];
      }
    },
  );
  originalAnimate = Object.getOwnPropertyDescriptor(
    HTMLElement.prototype,
    "animate",
  );
  Object.defineProperty(HTMLElement.prototype, "animate", {
    configurable: true,
    value: function (this: HTMLElement) {
      const animation = { cancel: vi.fn(), onfinish: null, oncancel: null };
      entrances.push({ message: this, cancel: animation.cancel });
      return animation;
    },
  });
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  if (originalAnimate)
    Object.defineProperty(HTMLElement.prototype, "animate", originalAnimate);
  else delete (HTMLElement.prototype as { animate?: unknown }).animate;
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("bottom chat motion", () => {
  it.each(["short", "scrolling"])(
    "eases growing %s conversations upward",
    (length) => {
      positions.set("user", 340);
      render([{ id: "user", role: "user", text: "Hello" }]);
      const view = viewport();
      if (length === "short") positions.set("user", 280);
      else scrollHeight += 60;
      view.resize();
      expect(offset(view.content)).toBe(60);
      expect(top).toBe(length === "short" ? 0 : 60);
      advance(40);
      const moving = offset(view.content);
      expect(moving).toBeGreaterThan(0);
      expect(moving).toBeLessThan(60);

      // A new line extends the in-flight movement, rather than restarting it.
      if (length === "short") positions.set("user", 250);
      else scrollHeight += 30;
      view.resize();
      expect(offset(view.content)).toBeCloseTo(moving + 30);
      expect(frames.size).toBe(1);
      for (let index = 0; index < 12; index++) advance(85);
      expect(view.content.style.transform).toBe("");
      expect(frames.size).toBe(0);
    },
  );

  it("keeps easing the reply's last lines after it finishes", () => {
    const blocks: Block[] = [{ id: "user", role: "user", text: "Hello" }];
    render(blocks);
    const view = viewport();
    scrollHeight += 40;
    view.resize();
    advance(40);
    const moving = offset(view.content);
    expect(moving).toBeGreaterThan(0);

    // The turn ends mid-motion, and its paced reveal is still growing.
    render(blocks, true, false);
    expect(offset(view.content)).toBeCloseTo(moving);
    scrollHeight += 20;
    view.resize();
    expect(offset(view.content)).toBeCloseTo(moving + 20);

    // Later changes outside a reply pin directly.
    now += 2000;
    scrollHeight += 20;
    view.resize();
    expect(view.content.style.transform).toBe("");
  });

  it("stops following when the reader scrolls back", () => {
    positions.set("user", 340);
    render([{ id: "user", role: "user", text: "Hello" }]);
    const view = viewport();
    scrollHeight = 500;
    view.resize();
    expect(frames.size).toBe(1);
    following.current = false;
    act(() =>
      view.scroller.dispatchEvent(new WheelEvent("wheel", { deltaY: -30 })),
    );
    expect(top).toBe(0);
    top = 60;
    scrollHeight = 550;
    view.resize();
    advance(85);
    expect(top).toBe(60);
    expect(view.content.style.transform).toBe("");
    expect(frames.size).toBe(0);
  });

  it("leaves settled upward scrolling to the browser without layout reads or scroll writes", () => {
    scrollHeight = 1000;
    positions.set("user", 900);
    render([{ id: "user", role: "user", text: "Hello" }]);
    const view = viewport();
    const anchorRect = vi.spyOn(
      view.content.lastElementChild!,
      "getBoundingClientRect",
    );
    const scrollerRect = vi.spyOn(view.scroller, "getBoundingClientRect");
    scrollWrites = [];

    for (const deltaY of [-4, -6, -12]) {
      act(() => {
        view.scroller.dispatchEvent(new WheelEvent("wheel", { deltaY }));
        // The browser applies the wheel's native scroll after dispatch.
        top += deltaY;
        view.scroller.dispatchEvent(new Event("scroll"));
      });
    }
    expect(top).toBe(578);
    expect(following.current).toBe(false);
    expect(scrollWrites).toEqual([]);
    expect(anchorRect).not.toHaveBeenCalled();
    expect(scrollerRect).not.toHaveBeenCalled();
    expect(frames.size).toBe(0);
  });

  it("hands off an in-flight offset once without measuring on wheel ticks", () => {
    scrollHeight = 1000;
    positions.set("user", 900);
    render([{ id: "user", role: "user", text: "Hello" }]);
    const view = viewport();
    scrollHeight += 80;
    view.resize();
    advance(40);
    const moving = offset(view.content);
    const visibleTop = top - moving;
    const anchorRect = vi.spyOn(
      view.content.lastElementChild!,
      "getBoundingClientRect",
    );
    scrollWrites = [];

    for (const deltaY of [-4, -6]) {
      act(() => {
        view.scroller.dispatchEvent(new WheelEvent("wheel", { deltaY }));
        top += deltaY;
        view.scroller.dispatchEvent(new Event("scroll"));
      });
    }
    expect(top).toBeCloseTo(visibleTop - 10);
    expect(scrollWrites).toEqual([visibleTop]);
    expect(anchorRect).not.toHaveBeenCalled();
    expect(view.content.style.transform).toBe("");
    advance(85);
    expect(top).toBeCloseTo(visibleTop - 10);
    expect(frames.size).toBe(0);
  });

  it("skips motion measurements for streamed updates while reading older messages", () => {
    const history: Block[] = [{ id: "user", role: "user", text: "Hello" }];
    scrollHeight = 1000;
    positions.set("user", 900);
    render(history);
    const view = viewport();
    act(() =>
      view.scroller.dispatchEvent(new WheelEvent("wheel", { deltaY: -100 })),
    );
    top = 300;
    const scrollerRect = vi.spyOn(view.scroller, "getBoundingClientRect");
    scrollWrites = [];

    scrollHeight += 100;
    positions.set("reply", 980);
    render([...history, { id: "reply", role: "assistant", text: "One" }]);
    view.resize();
    render([...history, { id: "reply", role: "assistant", text: "One two" }]);
    expect(top).toBe(300);
    expect(scrollWrites).toEqual([]);
    expect(scrollerRect).not.toHaveBeenCalled();
    expect(entrances).toHaveLength(0);

    top = 700;
    following.current = true;
    view.resize();
    expect(view.content.style.transform).toBe("");
    expect(entrances).toHaveLength(0);
  });

  it("does not animate a manual scroll back to the bottom", () => {
    const blocks: Block[] = [{ id: "user", role: "user", text: "Hello" }];
    scrollHeight = 1000;
    positions.set("user", 900);
    render(blocks);
    const view = viewport();
    act(() =>
      view.scroller.dispatchEvent(new WheelEvent("wheel", { deltaY: -100 })),
    );
    top = 300;
    act(() => view.scroller.dispatchEvent(new Event("scroll")));
    view.resize();

    top = 600;
    following.current = true;
    // A render can run before the browser delivers the queued scroll event.
    render([...blocks]);
    act(() => view.scroller.dispatchEvent(new Event("scroll")));
    view.resize();
    expect(top).toBe(600);
    expect(view.content.style.transform).toBe("");
    expect(frames.size).toBe(0);
  });

  it("resumes motion only for growth after returning to the bottom", () => {
    const blocks: Block[] = [{ id: "user", role: "user", text: "Hello" }];
    scrollHeight = 1000;
    positions.set("user", 900);
    render(blocks);
    const view = viewport();
    act(() =>
      view.scroller.dispatchEvent(new WheelEvent("wheel", { deltaY: -100 })),
    );
    top = 300;
    scrollHeight = 1100;
    view.resize();
    expect(top).toBe(300);

    top = 700;
    following.current = true;
    view.resize();
    expect(view.content.style.transform).toBe("");
    expect(frames.size).toBe(0);

    scrollHeight += 40;
    view.resize();
    expect(top).toBe(740);
    expect(offset(view.content)).toBe(40);
    advance(40);
    expect(offset(view.content)).toBeGreaterThan(0);
    expect(offset(view.content)).toBeLessThan(40);
  });

  it("animates new messages once without replaying loaded history or streamed tokens", () => {
    const history: Block[] = [{ id: "old", role: "user", text: "Earlier" }];
    render(history);
    viewport();
    expect(entrances).toHaveLength(0);
    const sent: Block[] = [
      ...history,
      { id: "new", role: "user", text: "Hello" },
    ];
    render(sent);
    render([...sent, { id: "reply", role: "assistant", text: "One" }]);
    render([...sent, { id: "reply", role: "assistant", text: "One two" }]);
    expect(entrances.map(({ message }) => message.dataset.chatMessage)).toEqual(
      ["new", "reply"],
    );
    render(sent, false);
    expect(
      entrances.every(({ cancel }) => cancel.mock.calls.length === 1),
    ).toBe(true);
    render(sent);
    expect(entrances).toHaveLength(2);
  });

  it("pins directly without animation when reduced motion is requested", () => {
    reduced = true;
    render([{ id: "user", role: "user", text: "Hello" }]);
    const view = viewport();
    scrollHeight = 550;
    view.resize();
    render([
      { id: "user", role: "user", text: "Hello" },
      { id: "reply", role: "assistant", text: "Hi" },
    ]);
    expect(top).toBe(150);
    expect(view.content.style.transform).toBe("");
    expect(frames.size).toBe(0);
    expect(entrances).toHaveLength(0);
  });

  it("skips entrances for newly loaded history while animating a live reply", () => {
    const prompt: Block = { id: "prompt", role: "user", text: "Hello" };
    render([prompt]);
    viewport();
    const history: Block[] = [
      { id: "old-prompt", role: "user", text: "Earlier question" },
      { id: "old-answer", role: "assistant", text: "Earlier answer" },
    ];
    render(
      [...history, prompt, { id: "reply", role: "assistant", text: "Hi" }],
      true,
      true,
      new Set(history.map((block) => block.id)),
    );
    expect(entrances.map(({ message }) => message.dataset.chatMessage)).toEqual(
      ["reply"],
    );
  });
});
