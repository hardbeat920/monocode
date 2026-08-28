import { useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { GitChangesPanel } from "../chrome/GitChangesPanel";
import type { HarnessId } from "../lib/session";

const MIN_WIDTH = 280;
const DEFAULT_WIDTH = 300;

let rememberedWidth = DEFAULT_WIDTH;

function clampDiffWidth(value: number) {
  const max = Math.min(
    Math.floor(window.innerWidth * 0.8),
    Math.max(MIN_WIDTH, window.innerWidth - 240),
  );
  return Math.min(max, Math.max(MIN_WIDTH, Math.round(value)));
}

type Props = {
  cwd: string;
  textHarness?: HarnessId;
  selectedPath?: string;
  sessionId?: string;
  focused: boolean;
  onFocus: () => void;
  onOpenFile: (path: string, sessionId?: string) => void;
};

export function DiffPane({
  cwd,
  textHarness,
  selectedPath,
  sessionId,
  focused,
  onFocus,
  onOpenFile,
}: Props) {
  const [width, setWidth] = useState(() => rememberedWidth);
  const [dragging, setDragging] = useState(false);
  const drag = useRef<{ startX: number; startW: number } | null>(null);
  const paneRef = useRef<HTMLElement>(null);
  const widthRef = useRef(width);
  const pendingWidth = useRef(width);
  const resizeFrame = useRef<number | null>(null);

  useEffect(() => {
    if (!dragging) return;
    const previous = document.body.style.cursor;
    document.body.style.cursor = "col-resize";
    return () => {
      document.body.style.cursor = previous;
    };
  }, [dragging]);

  useEffect(
    () => () => {
      if (resizeFrame.current != null) {
        cancelAnimationFrame(resizeFrame.current);
      }
    },
    [],
  );

  const applyWidth = (next: number, remember: boolean) => {
    pendingWidth.current = next;
    widthRef.current = next;
    if (remember) rememberedWidth = next;
    if (paneRef.current) paneRef.current.style.width = `${next}px`;
    setWidth(next);
  };

  const paintWidth = (next: number) => {
    pendingWidth.current = next;
    if (resizeFrame.current != null) return;
    resizeFrame.current = requestAnimationFrame(() => {
      resizeFrame.current = null;
      rememberedWidth = pendingWidth.current;
      if (paneRef.current) {
        paneRef.current.style.width = `${pendingWidth.current}px`;
      }
    });
  };

  const commitWidth = () => {
    if (resizeFrame.current != null) {
      cancelAnimationFrame(resizeFrame.current);
      resizeFrame.current = null;
    }
    applyWidth(pendingWidth.current, true);
  };

  const onResizePointerDown = (e: ReactPointerEvent<HTMLDivElement>) => {
    e.preventDefault();
    e.currentTarget.setPointerCapture(e.pointerId);
    drag.current = { startX: e.clientX, startW: widthRef.current };
    pendingWidth.current = widthRef.current;
    setDragging(true);
  };

  const onResizePointerMove = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (!drag.current) return;
    const next = clampDiffWidth(
      drag.current.startW - (e.clientX - drag.current.startX),
    );
    paintWidth(next);
  };

  const onResizePointerUp = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (!drag.current) return;
    drag.current = null;
    commitWidth();
    setDragging(false);
    if (e.currentTarget.hasPointerCapture(e.pointerId)) {
      e.currentTarget.releasePointerCapture(e.pointerId);
    }
  };

  return (
    <section
      ref={paneRef}
      data-diff-pane=""
      style={{ width }}
      className={`relative flex h-full min-h-0 shrink-0 flex-col border-l border-content/10 ${
        focused ? "bg-content/3" : "bg-content/2"
      }`}
      onMouseDown={onFocus}
    >
      <div
        role="separator"
        aria-orientation="vertical"
        aria-label="Resize changes pane"
        aria-valuenow={width}
        aria-valuemin={MIN_WIDTH}
        className={`absolute inset-y-0 -left-px z-10 w-1.5 cursor-col-resize touch-none ${
          dragging ? "bg-content/15" : "hover:bg-content/10"
        }`}
        onPointerDown={onResizePointerDown}
        onPointerMove={onResizePointerMove}
        onPointerUp={onResizePointerUp}
        onPointerCancel={onResizePointerUp}
        onDoubleClick={() => {
          pendingWidth.current = DEFAULT_WIDTH;
          commitWidth();
        }}
      />
      <GitChangesPanel
        cwd={cwd}
        enabled
        textHarness={textHarness}
        selectedPath={selectedPath}
        sessionId={sessionId}
        onOpenFile={onOpenFile}
      />
    </section>
  );
}
