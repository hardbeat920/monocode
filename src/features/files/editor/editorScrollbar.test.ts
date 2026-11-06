// @vitest-environment happy-dom
import { setDiagnostics } from "@codemirror/lint";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { afterEach, describe, expect, it } from "vitest";
import { schemeExtensions } from "./editorChrome";
import {
  diagnosticOverviewTicks,
  editorScrollbar,
  scrollbarMetrics,
} from "./editorScrollbar";

describe("scrollbarMetrics", () => {
  it("sizes and positions the thumb from the viewport", () => {
    expect(scrollbarMetrics(450, 1_000, 100, 200)).toEqual({
      scrollable: true,
      maxScroll: 900,
      maxThumbTop: 168,
      thumbHeight: 32,
      thumbTop: 84,
    });
  });

  it("fills the track when the document does not overflow", () => {
    expect(scrollbarMetrics(0, 100, 100, 200)).toEqual({
      scrollable: false,
      maxScroll: 0,
      maxThumbTop: 0,
      thumbHeight: 200,
      thumbTop: 0,
    });
  });

  it("clamps a stale scroll offset", () => {
    expect(scrollbarMetrics(2_000, 1_000, 200, 100).thumbTop).toBe(68);
  });
});

describe("diagnosticOverviewTicks", () => {
  it("maps diagnostics to the document and keeps the strongest per line", () => {
    let state = EditorState.create({ doc: "one\ntwo\nthree\nfour" });
    state = state.update(
      setDiagnostics(state, [
        { from: 4, to: 7, severity: "warning", message: "warning" },
        { from: 5, to: 6, severity: "error", message: "error" },
        { from: 14, to: 18, severity: "hint", message: "hint" },
      ]),
    ).state;

    expect(diagnosticOverviewTicks(state)).toEqual([
      { severity: "error", top: 1 / 4, pos: 5, message: "error" },
      { severity: "hint", top: 3 / 4, pos: 14, message: "hint" },
    ]);
  });
});

describe("editorScrollbar rail", () => {
  let view: EditorView | null = null;
  const RAIL = 18;
  const EDITOR_LEFT = 100;
  const EDITOR_RIGHT = 400;

  afterEach(() => {
    view?.destroy();
    view = null;
  });

  /** happy-dom reports 0 for every layout box, so the sizes are declared. */
  async function mount(scrollHeight: number) {
    view = new EditorView({ doc: "one", extensions: [editorScrollbar] });
    const box = (left: number, right: number, top = 0, bottom = 200) => ({
      left,
      right,
      top,
      bottom,
      width: right - left,
      height: bottom - top,
    });
    for (const [el, rect] of [
      [view.dom, box(EDITOR_LEFT, EDITOR_RIGHT)],
      [
        view.scrollDOM,
        { ...box(EDITOR_LEFT, EDITOR_RIGHT), clientHeight: 200 },
      ],
    ] as const) {
      Object.defineProperty(el, "getBoundingClientRect", { value: () => rect });
    }
    const scroller = view.scrollDOM;
    for (const [name, value] of [
      ["clientHeight", 200],
      ["scrollHeight", scrollHeight],
      ["scrollTop", 0],
      ["offsetTop", 0],
      ["offsetHeight", 200],
    ] as const) {
      Object.defineProperty(scroller, name, {
        value,
        writable: name === "scrollTop",
        configurable: true,
      });
    }
    const rail = view.dom.querySelector(".cm-editorScrollbar")!;
    Object.defineProperty(rail, "clientHeight", {
      value: 200,
      configurable: true,
    });
    Object.defineProperty(rail, "getBoundingClientRect", {
      value: () => box(EDITOR_RIGHT - RAIL, EDITOR_RIGHT),
    });
    return { scroller, rail, inRail: EDITOR_RIGHT - RAIL + 4, inText: 200 };
  }

  function cssRules() {
    return Array.from(document.querySelectorAll("style"))
      .flatMap((s) => (s.textContent ?? "").split("}"))
      .filter(Boolean);
  }

  /** The strip is measured in a scheduled frame, so let one run. */
  function frame() {
    return new Promise((resolve) =>
      requestAnimationFrame(() => requestAnimationFrame(() => resolve(null))),
    );
  }

  /** happy-dom's WheelEvent drops clientX from the init, so it is set here. */
  function wheelOn(target: Element, deltaY: number, clientX: number) {
    const event = new WheelEvent("wheel", {
      deltaY,
      bubbles: true,
      cancelable: true,
    });
    Object.defineProperty(event, "clientX", { value: clientX });
    target.dispatchEvent(event);
    return event;
  }

  function pointerOn(
    target: Element,
    type: string,
    clientX: number,
    clientY = 0,
    init: PointerEventInit = {},
  ) {
    const event = new PointerEvent(type, {
      clientX,
      clientY,
      button: 0,
      pointerId: 1,
      bubbles: true,
      cancelable: true,
      ...init,
    });
    target.dispatchEvent(event);
    return event;
  }

  it("is see-through, so the wheel belongs to the scroller underneath", async () => {
    const { scroller, rail, inRail } = await mount(1_000);
    await frame();
    expect(rail.parentElement).toBe(view!.dom);
    expect(scroller.contains(rail)).toBe(false);
    // The browser never targets a pointer-events: none element, so a wheel
    // over the rail strip lands on the scroller and scrolls natively.
    expect(wheelOn(scroller, 120, inRail).defaultPrevented).toBe(false);
    expect(scroller.scrollTop).toBe(0);
  });

  it("declares the rail see-through in its own stylesheet", async () => {
    await mount(1_000);
    const rule = cssRules().find(
      (r) =>
        r.includes(".cm-editorScrollbar {") && r.includes("pointer-events"),
    );
    expect(rule).toBeDefined();
    expect(rule).toContain("pointer-events: none");
  });

  it("forwards a wheel that lands on a diff tick", async () => {
    const { scroller, inRail } = await mount(1_000);
    await frame();
    const tick = document.createElement("div");
    tick.className = "cm-gitOverviewTick";
    view!.dom.append(tick);

    expect(wheelOn(tick, 120, inRail).defaultPrevented).toBe(true);
    expect(scroller.scrollTop).toBe(120);
    expect(wheelOn(tick, -60, inRail).defaultPrevented).toBe(true);
    expect(scroller.scrollTop).toBe(60);
  });

  it("leaves a wheel over the search panel to the browser", async () => {
    const { scroller, inRail } = await mount(1_000);
    await frame();
    const panel = document.createElement("div");
    panel.className = "cm-panels";
    view!.dom.append(panel);

    const event = wheelOn(panel, 120, inRail);
    expect(event.defaultPrevented).toBe(false);
    expect(scroller.scrollTop).toBe(0);
  });

  it("lets a document that does not overflow pass the wheel on", async () => {
    const { scroller, inRail } = await mount(200);
    await frame();
    const tick = document.createElement("div");
    tick.className = "cm-gitOverviewTick";
    view!.dom.append(tick);

    expect(wheelOn(tick, 120, inRail).defaultPrevented).toBe(false);
    expect(scroller.scrollTop).toBe(0);
  });

  it("marks the rail hovered from the pointer's own coordinates", async () => {
    const { rail, scroller, inRail, inText } = await mount(1_000);
    await frame();
    expect(rail.hasAttribute("data-hover")).toBe(false);

    pointerOn(scroller, "pointermove", inRail);
    expect(rail.hasAttribute("data-hover")).toBe(true);
    expect(scroller.style.cursor).toBe("default");

    pointerOn(scroller, "pointermove", inText);
    expect(rail.hasAttribute("data-hover")).toBe(false);
    expect(scroller.style.cursor).toBe("");
  });

  it("restores whatever cursor the scroller already had", async () => {
    const { scroller, inRail } = await mount(1_000);
    await frame();
    scroller.style.cursor = "crosshair";
    pointerOn(scroller, "pointermove", inRail);
    expect(scroller.style.cursor).toBe("default");
    pointerOn(scroller, "pointermove", inRail - 100);
    expect(scroller.style.cursor).toBe("crosshair");
  });

  it("drags the thumb from a press inside the rail strip", async () => {
    const { rail, scroller, inRail } = await mount(1_000);
    await frame();
    // Below the thumb, so this is a track click and must page the editor.
    const down = pointerOn(scroller, "pointerdown", inRail, 120);
    // The caret must not move: the press is swallowed before mousedown.
    expect(down.defaultPrevented).toBe(true);
    expect(rail.dataset.dragging).toBe("true");
    expect(scroller.scrollTop).toBeGreaterThan(0);

    const grabbed = scroller.scrollTop;
    pointerOn(view!.dom, "pointermove", inRail, 180);
    expect(scroller.scrollTop).toBeGreaterThan(grabbed);
    expect(rail.dataset.dragging).toBe("true");

    pointerOn(view!.dom, "pointerup", inRail, 180);
    expect(rail.dataset.dragging).toBeUndefined();
  });

  it("leaves a touch press to the native pan of the see-through strip", async () => {
    const { rail, scroller, inRail } = await mount(1_000);
    await frame();
    const down = pointerOn(scroller, "pointerdown", inRail, 120, {
      pointerType: "touch",
    });
    expect(down.defaultPrevented).toBe(false);
    expect(rail.dataset.dragging).toBeUndefined();
  });

  it("leaves a press in the text to the editor", async () => {
    const { rail, scroller, inText } = await mount(1_000);
    await frame();
    const down = pointerOn(scroller, "pointerdown", inText, 40);
    expect(down.defaultPrevented).toBe(false);
    expect(rail.dataset.dragging).toBeUndefined();
  });

  it("leaves the diff overview its own press, which lives in the same strip", async () => {
    const { rail, inRail } = await mount(1_000);
    await frame();
    // editorGit puts .cm-gitOverview at right: 0, width 18px: every pixel of
    // it is inside the rail strip, so the coordinate test alone would eat it.
    const git = document.createElement("div");
    git.className = "cm-gitOverview";
    let pressed = false;
    git.addEventListener("mousedown", () => {
      pressed = true;
    });
    view!.dom.append(git);

    const down = pointerOn(git, "pointerdown", inRail, 40);
    expect(down.defaultPrevented).toBe(false);
    expect(rail.dataset.dragging).toBeUndefined();
    git.dispatchEvent(
      new MouseEvent("mousedown", {
        clientX: inRail,
        clientY: 40,
        button: 0,
        bubbles: true,
        cancelable: true,
      }),
    );
    expect(pressed).toBe(true);
  });

  it("keeps the caret out of the strip, so a line never runs under the rail", () => {
    const host = document.createElement("div");
    document.body.append(host);
    view = new EditorView({
      doc: "one",
      parent: host,
      // The order FileEditor uses: the chrome theme before the scrollbar one.
      extensions: [schemeExtensions("dark"), editorScrollbar],
    });
    expect(getComputedStyle(view.contentDOM).paddingRight).toBe(`${RAIL}px`);
  });

  it("still jumps the track when the platform refuses the pointer capture", async () => {
    const { rail, scroller, inRail } = await mount(1_000);
    await frame();
    // The spec has setPointerCapture throw when the id is no longer active,
    // and happy-dom never throws on its own, so it is provoked here.
    view!.dom.setPointerCapture = () => {
      throw new Error("pointer is not active");
    };

    expect(() => pointerOn(scroller, "pointerdown", inRail, 120)).not.toThrow();
    expect(scroller.scrollTop).toBeGreaterThan(0);
    // Nothing holds the capture, so nothing must be left believing it does.
    expect(rail.dataset.dragging).toBeUndefined();

    // And the next press still works.
    pointerOn(scroller, "pointerdown", inRail, 40);
    expect(rail.dataset.dragging).toBeUndefined();
  });

  it("sizes the rail to the scroller, not the editor, so a panel cannot clip it", async () => {
    const { rail, scroller } = await mount(1_000);
    Object.defineProperty(scroller, "offsetTop", {
      value: 44,
      configurable: true,
    });
    Object.defineProperty(scroller, "offsetHeight", {
      value: 156,
      configurable: true,
    });
    scroller.dispatchEvent(new Event("scroll"));
    await frame();
    expect(rail.style.top).toBe("44px");
    expect(rail.style.height).toBe("156px");
  });

  it("releases the rail when the window loses focus mid-drag", async () => {
    const { rail, scroller, inRail } = await mount(1_000);
    await frame();
    pointerOn(scroller, "pointerdown", inRail, 120);
    expect(rail.dataset.dragging).toBe("true");

    // No pointerup: the gesture ended because the window went away.
    window.dispatchEvent(new Event("blur"));
    expect(rail.dataset.dragging).toBeUndefined();
    expect(rail.hasAttribute("data-hover")).toBe(false);

    // And the rail must still work afterwards.
    const down = pointerOn(scroller, "pointerdown", inRail, 120);
    expect(down.defaultPrevented).toBe(true);
    expect(rail.dataset.dragging).toBe("true");
  });

  it("stops listening once the view is destroyed", async () => {
    const { scroller, rail, inRail } = await mount(1_000);
    await frame();
    const dom = view!.dom;
    view!.destroy();
    view = null;

    // Armed, so a surviving listener would visibly disarm it.
    rail.dataset.dragging = "true";
    const down = pointerOn(scroller, "pointerdown", inRail, 120);
    expect(down.defaultPrevented).toBe(false);
    expect(dom.querySelector(".cm-editorScrollbar")).toBeNull();
    // The window listener has to go too, or a later blur would still reach a
    // destroyed view.
    window.dispatchEvent(new Event("blur"));
    expect(rail.dataset.dragging).toBe("true");
  });
});
