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

  constructor(readonly view: EditorView) {
    this.dom.className = "cm-editorScrollbar";
    this.dom.setAttribute("aria-hidden", "true");
    this.markers.className = "cm-editorScrollbarMarkers";
    this.thumb.className = "cm-editorScrollbarThumb";
    this.cursor.className = "cm-editorScrollbarCursor";
    this.dom.append(this.thumb, this.markers, this.cursor);
    this.view.dom.appendChild(this.dom);

    this.dom.addEventListener("pointerdown", this.onPointerDown);
    this.dom.addEventListener("pointermove", this.onPointerMove);
    this.dom.addEventListener("pointerup", this.onPointerUp);
    this.dom.addEventListener("pointercancel", this.onPointerUp);
    this.dom.addEventListener("lostpointercapture", this.onLostPointerCapture);
    this.view.scrollDOM.addEventListener("scroll", this.onScroll, {
      passive: true,
    });

    this.resizeObserver =
      typeof ResizeObserver === "undefined"
        ? null
        : new ResizeObserver(this.scheduleDraw);
    this.resizeObserver?.observe(this.view.scrollDOM);
    this.drawMarkers();
    this.drawCursor();
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
    this.dom.removeEventListener("pointerdown", this.onPointerDown);
    this.dom.removeEventListener("pointermove", this.onPointerMove);
    this.dom.removeEventListener("pointerup", this.onPointerUp);
    this.dom.removeEventListener("pointercancel", this.onPointerUp);
    this.dom.removeEventListener(
      "lostpointercapture",
      this.onLostPointerCapture,
    );
    this.dom.remove();
  }

  private readonly onScroll = () => this.scheduleDraw();

  private readonly scheduleDraw = () => {
    if (this.frame) return;
    this.frame = requestAnimationFrame(() => {
      this.frame = 0;
      this.drawThumb();
    });
  };

  private readonly onPointerDown = (event: PointerEvent) => {
    if (event.button !== 0 || this.pointerId !== null) return;
    event.preventDefault();
    event.stopPropagation();

    const target = event.target;
    if (target instanceof HTMLElement) {
      const tick = target.closest<HTMLElement>(".cm-editorScrollbarTick");
      const pos = tick?.dataset.pos;
      if (pos != null) {
        const anchor = Number(pos);
        this.view.dispatch({
          selection: { anchor },
          effects: EditorView.scrollIntoView(anchor, { y: "center" }),
        });
        this.view.focus();
        return;
      }
    }

    const rect = this.dom.getBoundingClientRect();
    const metrics = this.metrics();
    if (!metrics.scrollable) return;
    this.pointerId = event.pointerId;
    this.dom.dataset.dragging = "true";
    this.dragOffset =
      event.target === this.thumb
        ? event.clientY - rect.top - metrics.thumbTop
        : metrics.thumbHeight / 2;
    this.dom.setPointerCapture?.(event.pointerId);
    this.scrollFromPointer(event.clientY);
  };

  private readonly onPointerMove = (event: PointerEvent) => {
    if (event.pointerId !== this.pointerId) return;
    event.preventDefault();
    this.scrollFromPointer(event.clientY);
  };

  private readonly onPointerUp = (event: PointerEvent) => {
    if (event.pointerId !== this.pointerId) return;
    if (this.dom.hasPointerCapture?.(event.pointerId)) {
      this.dom.releasePointerCapture(event.pointerId);
    }
    this.stopDragging();
  };

  private readonly onLostPointerCapture = () => this.stopDragging();

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
    touchAction: "none",
    userSelect: "none",
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
    cursor: "default",
    willChange: "transform",
  },
  ".cm-editorScrollbar:hover .cm-editorScrollbarThumb, .cm-editorScrollbar[data-dragging] .cm-editorScrollbarThumb":
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
