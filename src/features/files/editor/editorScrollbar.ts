import {
  forEachDiagnostic,
  setDiagnosticsEffect,
  type Diagnostic,
} from "@codemirror/lint";
import type { EditorState, Extension } from "@codemirror/state";
import { EditorView, ViewPlugin, type ViewUpdate } from "@codemirror/view";

const RAIL_WIDTH = 18;
const MIN_THUMB_HEIGHT = 32;

export type ScrollbarMetrics = {
  scrollable: boolean;
  maxScroll: number;
  maxThumbTop: number;
  thumbHeight: number;
  thumbTop: number;
};

export function scrollbarMetrics(
  scrollTop: number,
  scrollHeight: number,
  clientHeight: number,
  trackHeight: number,
): ScrollbarMetrics {
  const viewport = Math.max(0, clientHeight);
  const content = Math.max(viewport, scrollHeight);
  const track = Math.max(0, trackHeight);
  const maxScroll = Math.max(0, content - viewport);
  if (track === 0 || maxScroll === 0) {
    return {
      scrollable: false,
      maxScroll,
      maxThumbTop: 0,
      thumbHeight: track,
      thumbTop: 0,
    };
  }

  const thumbHeight = Math.min(
    track,
    Math.max(MIN_THUMB_HEIGHT, track * (viewport / content)),
  );
  const maxThumbTop = Math.max(0, track - thumbHeight);
  const progress = Math.min(1, Math.max(0, scrollTop / maxScroll));
  return {
    scrollable: true,
    maxScroll,
    maxThumbTop,
    thumbHeight,
    thumbTop: progress * maxThumbTop,
  };
}

export type DiagnosticOverviewTick = {
  severity: Diagnostic["severity"];
  top: number;
  pos: number;
  message: string;
};

const SEVERITY_PRIORITY: Record<Diagnostic["severity"], number> = {
  hint: 0,
  info: 1,
  warning: 2,
  error: 3,
};

/** One marker per line, keeping the most important diagnostic on that line. */
export function diagnosticOverviewTicks(
  state: EditorState,
): DiagnosticOverviewTick[] {
  const ticks = new Map<number, DiagnosticOverviewTick>();
  const lineCount = Math.max(1, state.doc.lines);
  forEachDiagnostic(state, (diagnostic, from) => {
    const pos = Math.min(state.doc.length, Math.max(0, from));
    const line = state.doc.lineAt(pos).number;
    const next = {
      severity: diagnostic.severity,
      top: (line - 1) / lineCount,
      pos,
      message: diagnostic.message,
    };
    const current = ticks.get(line);
    if (
      !current ||
      SEVERITY_PRIORITY[next.severity] > SEVERITY_PRIORITY[current.severity]
    ) {
      ticks.set(line, next);
    }
  });
  return [...ticks.values()].sort((a, b) => a.pos - b.pos);
}

class EditorScrollbar {
  readonly dom = document.createElement("div");
  readonly markers = document.createElement("div");
  readonly thumb = document.createElement("div");
  readonly cursor = document.createElement("div");
  readonly resizeObserver: ResizeObserver | null;
  private frame = 0;
  private pointerId: number | null = null;
  private dragOffset = 0;
  private hovered = false;
  private savedCursor = "";
  private railLeft = 0;
  private railRight = 0;
  private railTop = -1;
  private railHeight = -1;

  constructor(readonly view: EditorView) {
    this.dom.className = "cm-editorScrollbar";
    this.dom.setAttribute("aria-hidden", "true");
    this.markers.className = "cm-editorScrollbarMarkers";
    this.thumb.className = "cm-editorScrollbarThumb";
    this.cursor.className = "cm-editorScrollbarCursor";
    this.dom.append(this.thumb, this.markers, this.cursor);
    // The rail is a sibling of the scroller, so a wheel over it has no
    // scrollable ancestor and the browser would drop the gesture. Making it
    // transparent to hit-testing hands the wheel to the scroller underneath,
    // which is the only way to scroll the editor exactly as the text does.
    // Everything the rail needs from the pointer is read off the coordinates.
    this.view.dom.appendChild(this.dom);

    this.view.dom.addEventListener("wheel", this.onWheel, { passive: false });
    this.view.dom.addEventListener("pointerdown", this.onPointerDown, true);
    this.view.dom.addEventListener("pointermove", this.onPointerMove, true);
    this.view.dom.addEventListener("pointerup", this.onPointerUp, true);
    this.view.dom.addEventListener("pointercancel", this.onPointerUp, true);
    this.view.dom.addEventListener(
      "lostpointercapture",
      this.onLostPointerCapture,
    );
    this.view.dom.addEventListener("pointerleave", this.onPointerLeave);
    // Belt and braces: the press is suppressed on pointerdown, but a press
    // that slipped through would put the caret in the rail strip.
    this.view.dom.addEventListener("mousedown", this.onMouseDown, true);
    this.view.scrollDOM.addEventListener("scroll", this.onScroll, {
      passive: true,
    });
    window.addEventListener("blur", this.onWindowBlur);

    this.resizeObserver =
      typeof ResizeObserver === "undefined"
        ? null
        : new ResizeObserver(this.scheduleDraw);
    this.resizeObserver?.observe(this.view.scrollDOM);
    this.drawMarkers();
    this.drawCursor();
    // Seeded now, so a wheel or press in the first frame is not measured
    // against an empty strip.
    this.measure();
    this.scheduleDraw();
  }

  update(update: ViewUpdate) {
    const diagnosticsChanged = update.transactions.some((transaction) =>
      transaction.effects.some((effect) => effect.is(setDiagnosticsEffect)),
    );
    if (update.docChanged || diagnosticsChanged) this.drawMarkers();
    if (update.docChanged || update.selectionSet) this.drawCursor();
    if (update.docChanged || update.geometryChanged) this.scheduleDraw();
  }

  destroy() {
    if (this.frame) cancelAnimationFrame(this.frame);
    this.resizeObserver?.disconnect();
    this.view.scrollDOM.removeEventListener("scroll", this.onScroll);
    this.view.dom.removeEventListener("wheel", this.onWheel);
    this.view.dom.removeEventListener("pointerdown", this.onPointerDown, true);
    this.view.dom.removeEventListener("pointermove", this.onPointerMove, true);
    this.view.dom.removeEventListener("pointerup", this.onPointerUp, true);
    this.view.dom.removeEventListener("pointercancel", this.onPointerUp, true);
    this.view.dom.removeEventListener(
      "lostpointercapture",
      this.onLostPointerCapture,
    );
    this.view.dom.removeEventListener("pointerleave", this.onPointerLeave);
    this.view.dom.removeEventListener("mousedown", this.onMouseDown, true);
    window.removeEventListener("blur", this.onWindowBlur);
    this.setHover(false);
    this.dom.remove();
  }

  private readonly onScroll = () => this.scheduleDraw();

  /**
   * The diff overview overlay and the diagnostic ticks are the only things
   * still inside the rail strip that are not the scroller, and neither is a
   * scrollable ancestor of the wheel. Everything else in the strip - the
   * search panel, a hunk bar - is left to the browser, so a wheel over a
   * nested scroller there is not hijacked.
   */
  private readonly onWheel = (event: WheelEvent) => {
    if (event.deltaY === 0) return;
    const target = event.target;
    if (!(target instanceof Element)) return;
    if (!this.overRail(event.clientX)) return;
    if (
      !target.matches(
        ".cm-editorScrollbarTick, .cm-gitOverviewTick, .cm-gitOverview",
      )
    ) {
      return;
    }
    const scroller = this.view.scrollDOM;
    // A document that does not overflow has nothing to offer, so the
    // surrounding pane still takes the wheel.
    const max = scroller.scrollHeight - scroller.clientHeight;
    if (max <= 0) return;
    const scale =
      event.deltaMode === 1
        ? this.view.defaultLineHeight
        : event.deltaMode === 2
          ? scroller.clientHeight
          : 1;
    event.preventDefault();
    scroller.scrollTop = Math.min(
      max,
      Math.max(0, scroller.scrollTop + event.deltaY * scale),
    );
  };

  private readonly scheduleDraw = () => {
    if (this.frame) return;
    this.frame = requestAnimationFrame(() => {
      this.frame = 0;
      // Both reads, once per frame. The rail never moves against the viewport,
      // so caching them keeps a pointermove from forcing a layout of its own.
      this.measure();
      this.drawThumb();
    });
  };

  private overRail(clientX: number) {
    return clientX >= this.railLeft && clientX <= this.railRight;
  }

  /**
   * The rail is see-through, so it has no box of its own to measure. Its strip
   * is the editor's own right edge, which is where it is painted.
   */
  private measure() {
    this.railLeft = this.view.dom.getBoundingClientRect().right - RAIL_WIDTH;
    this.railRight = this.railLeft + RAIL_WIDTH;
  }

  /**
   * The search panel is in flow, so the scroller does not start at the top of
   * the editor. The rail has to follow it, or the thumb is sized against a
   * track that runs on under the panel.
   */
  private syncRailBox() {
    const scroller = this.view.scrollDOM;
    const top = scroller.offsetTop;
    const height = scroller.offsetHeight;
    if (this.railTop === top && this.railHeight === height) return;
    this.railTop = top;
    this.railHeight = height;
    this.dom.style.top = `${top}px`;
    this.dom.style.height = `${height}px`;
  }

  /**
   * Only a press that reached the editor's own content is the rail's to take.
   * Anything else in the strip - the diff overview overlay, the search panel,
   * a hunk bar - is a real control that the browser hit-tested on purpose, and
   * swallowing it here would strand it.
   */
  private claimsPress(target: EventTarget | null) {
    return target instanceof Node && this.view.scrollDOM.contains(target);
  }

  private readonly onPointerDown = (event: PointerEvent) => {
    if (event.button !== 0 || this.pointerId !== null) return;
    // A gesture is worth a fresh read: the cached strip is one frame old and
    // the window may have moved since.
    this.measure();

    const target = event.target instanceof Element ? event.target : null;
    const tick = target?.closest<HTMLElement>(".cm-editorScrollbarTick");
    const pos = tick?.dataset.pos;
    if (pos != null) {
      event.preventDefault();
      event.stopPropagation();
      const anchor = Number(pos);
      this.view.dispatch({
        selection: { anchor },
        effects: EditorView.scrollIntoView(anchor, { y: "center" }),
      });
      this.view.focus();
      return;
    }

    if (!this.claimsPress(event.target)) return;
    if (!this.overRail(event.clientX)) return;
    // A finger pans the see-through strip natively. Claiming it here too would
    // leave two writers on scrollTop.
    if (event.pointerType === "touch") return;
    // Suppresses the compatibility mousedown, so the caret stays put.
    event.preventDefault();
    event.stopPropagation();

    const rect = this.dom.getBoundingClientRect();
    const metrics = this.metrics();
    if (!metrics.scrollable) return;
    this.pointerId = event.pointerId;
    this.dom.dataset.dragging = "true";
    const top = event.clientY - rect.top;
    const onThumb =
      top >= metrics.thumbTop && top <= metrics.thumbTop + metrics.thumbHeight;
    this.dragOffset = onThumb
      ? top - metrics.thumbTop
      : metrics.thumbHeight / 2;
    this.scrollFromPointer(event.clientY);
    // After the first jump, so a failure here costs the capture and not the
    // click. An id the platform no longer considers active throws, and that
    // would otherwise leave pointerId set with no way left to clear it.
    try {
      this.view.dom.setPointerCapture(event.pointerId);
    } catch {
      this.stopDragging();
    }
  };

  private readonly onPointerMove = (event: PointerEvent) => {
    if (event.pointerId === this.pointerId) {
      event.preventDefault();
      this.scrollFromPointer(event.clientY);
      return;
    }
    this.setHover(
      this.claimsPress(event.target) && this.overRail(event.clientX),
    );
  };

  private readonly onPointerUp = (event: PointerEvent) => {
    if (event.pointerId !== this.pointerId) return;
    if (this.view.dom.hasPointerCapture(event.pointerId)) {
      this.view.dom.releasePointerCapture(event.pointerId);
    }
    this.stopDragging();
  };

  private readonly onPointerLeave = () => this.setHover(false);

  private readonly onMouseDown = (event: MouseEvent) => {
    if (event.button !== 0 || !this.claimsPress(event.target)) return;
    if (!this.overRail(event.clientX)) return;
    event.preventDefault();
    event.stopPropagation();
  };

  private readonly onLostPointerCapture = () => this.stopDragging();

  /**
   * A drag must never outlive the gesture, or the pointerId gate above would
   * refuse every later press and the rail would be dead until the view is
   * rebuilt. Alt-Tab, a Mission Control swipe or a release over another window
   * all end the pointer without a pointerup.
   */
  private readonly onWindowBlur = () => {
    this.setHover(false);
    this.stopDragging();
  };

  private setHover(hover: boolean) {
    if (hover === this.hovered) return;
    this.hovered = hover;
    this.dom.toggleAttribute("data-hover", hover);
    // The rail is see-through, so the scroller decides the cursor. Saved and
    // restored rather than blanked, so nothing else can write here.
    if (hover) {
      this.savedCursor = this.view.scrollDOM.style.cursor;
      this.view.scrollDOM.style.cursor = "default";
    } else {
      this.view.scrollDOM.style.cursor = this.savedCursor;
    }
  }

  private stopDragging() {
    this.pointerId = null;
    delete this.dom.dataset.dragging;
  }

  private metrics(): ScrollbarMetrics {
    const scroller = this.view.scrollDOM;
    return scrollbarMetrics(
      scroller.scrollTop,
      scroller.scrollHeight,
      scroller.clientHeight,
      this.dom.clientHeight,
    );
  }

  private scrollFromPointer(clientY: number) {
    const metrics = this.metrics();
    if (!metrics.scrollable || metrics.maxThumbTop === 0) return;
    const rect = this.dom.getBoundingClientRect();
    const thumbTop = Math.min(
      metrics.maxThumbTop,
      Math.max(0, clientY - rect.top - this.dragOffset),
    );
    this.view.scrollDOM.scrollTop =
      (thumbTop / metrics.maxThumbTop) * metrics.maxScroll;
  }

  private drawThumb() {
    this.syncRailBox();
    const metrics = this.metrics();
    this.thumb.hidden = !metrics.scrollable;
    if (!metrics.scrollable) return;
    this.thumb.style.height = `${metrics.thumbHeight}px`;
    this.thumb.style.transform = `translate3d(0, ${metrics.thumbTop}px, 0)`;
  }

  private drawCursor() {
    const state = this.view.state;
    const line = state.doc.lineAt(state.selection.main.head).number;
    const top = (line - 1) / Math.max(1, state.doc.lines);
    this.cursor.style.top = `${top * 100}%`;
  }

  private drawMarkers() {
    const fragment = document.createDocumentFragment();
    for (const tick of diagnosticOverviewTicks(this.view.state)) {
      const marker = document.createElement("div");
      marker.className = `cm-editorScrollbarTick cm-editorScrollbar-${tick.severity}`;
      marker.style.top = `${tick.top * 100}%`;
      marker.dataset.pos = String(tick.pos);
      marker.title = tick.message;
      fragment.appendChild(marker);
    }
    this.markers.replaceChildren(fragment);
  }
}

const scrollbarPlugin = ViewPlugin.fromClass(EditorScrollbar);

const scrollbarTheme = EditorView.theme({
  "&": {
    position: "relative",
    "--editor-scrollbar-width": `${RAIL_WIDTH}px`,
  },
  ".cm-scroller": {
    scrollbarWidth: "none",
  },
  ".cm-scroller::-webkit-scrollbar": {
    width: "0",
    height: "0",
  },
  ".cm-content": {
    paddingRight: `${RAIL_WIDTH}px`,
  },
  ".cm-editorScrollbar": {
    position: "absolute",
    zIndex: "12",
    top: "0",
    right: "0",
    bottom: "0",
    width: `${RAIL_WIDTH}px`,
    boxSizing: "border-box",
    borderLeft:
      "1px solid color-mix(in srgb, var(--color-content) 7%, transparent)",
    background: "transparent",
    cursor: "default",
    userSelect: "none",
    // See-through, so the scroller below keeps the wheel. The pointer is read
    // off the coordinates instead.
    pointerEvents: "none",
  },
  ".cm-editorScrollbarMarkers": {
    position: "absolute",
    inset: "0",
    pointerEvents: "none",
  },
  ".cm-editorScrollbarThumb": {
    position: "absolute",
    zIndex: "1",
    top: "0",
    left: "3px",
    right: "3px",
    minHeight: `${MIN_THUMB_HEIGHT}px`,
    borderRadius: "2px",
    backgroundColor:
      "color-mix(in srgb, var(--color-content) 30%, transparent)",
    opacity: "0.72",
    willChange: "transform",
  },
  ".cm-editorScrollbar[data-hover] .cm-editorScrollbarThumb, .cm-editorScrollbar[data-dragging] .cm-editorScrollbarThumb":
    {
      backgroundColor:
        "color-mix(in srgb, var(--color-content) 38%, transparent)",
      opacity: "0.88",
    },
  ".cm-editorScrollbarTick": {
    position: "absolute",
    zIndex: "2",
    left: "3px",
    right: "2px",
    height: "3px",
    minHeight: "3px",
    borderRadius: "1px 0 0 1px",
    pointerEvents: "auto",
  },
  ".cm-editorScrollbar-error": {
    backgroundColor: "#f87171",
  },
  ".cm-editorScrollbar-warning": {
    backgroundColor: "#fbbf24",
  },
  ".cm-editorScrollbar-info": {
    backgroundColor: "#60a5fa",
  },
  ".cm-editorScrollbar-hint": {
    backgroundColor:
      "color-mix(in srgb, var(--color-content) 48%, transparent)",
  },
  ".cm-editorScrollbarCursor": {
    position: "absolute",
    zIndex: "3",
    left: "0",
    right: "0",
    height: "2px",
    pointerEvents: "none",
    backgroundColor: "var(--color-accent)",
    boxShadow:
      "0 0 0 1px color-mix(in srgb, var(--color-background-base) 42%, transparent)",
  },
  "&:not(.cm-focused) .cm-editorScrollbarCursor": {
    opacity: "0.6",
  },
});

export const editorScrollbar: Extension = [scrollbarPlugin, scrollbarTheme];
