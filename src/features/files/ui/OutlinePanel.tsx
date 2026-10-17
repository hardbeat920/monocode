import {
  useEffect,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
} from "react";
import { GlassBackdrop } from "../../../app/shell/GlassBackdrop";
import { LAYER } from "../../../shared/lib/layers";
import { suppressTextSelection } from "../../../shared/lib/drag";
import {
  AppWindow,
  Braces,
  Cube,
  Function as FunctionIcon,
  GripVertical,
  Heading,
  Layers,
  ListBullet,
  Paragraph,
  PanelRight,
  Structure,
  Variable,
  X,
  type IconComponent,
} from "../../../shared/ui/icons";
import type { OutlineItem, OutlineKind } from "../editor/editorOutline";

const FLOAT_WIDTH = 264;
const FLOAT_MARGIN = 8;

const KIND_ICONS: Record<OutlineKind, IconComponent> = {
  function: FunctionIcon,
  method: Braces,
  class: Structure,
  interface: Structure,
  type: Structure,
  enum: Structure,
  variable: Variable,
  constant: Variable,
  property: Cube,
  heading: Heading,
  module: Layers,
  namespace: Layers,
  section: Paragraph,
  other: ListBullet,
};

const KIND_TINTS: Record<OutlineKind, string> = {
  function: "text-sky-400/80",
  method: "text-sky-400/70",
  class: "text-amber-400/80",
  interface: "text-amber-400/80",
  type: "text-amber-400/70",
  enum: "text-amber-400/70",
  variable: "text-emerald-400/80",
  constant: "text-emerald-400/70",
  property: "text-violet-400/80",
  heading: "text-content/45",
  module: "text-content/45",
  namespace: "text-content/45",
  section: "text-content/45",
  other: "text-content/40",
};

export type OutlineResize = {
  setPaneRef: (el: HTMLElement | null) => void;
  onPointerDown: (event: ReactPointerEvent<HTMLElement>) => void;
  onDoubleClick: () => void;
  dragging: boolean;
};

type PanelProps = {
  items: OutlineItem[];
  activeId: string | null;
  onSelect: (item: OutlineItem) => void;
  onClose: () => void;
  onDetach: () => void;
  resize: OutlineResize;
};

/** The docked outline column: a resizable strip pinned to the editor's right. */
export function OutlinePanel({
  items,
  activeId,
  onSelect,
  onClose,
  onDetach,
  resize,
}: PanelProps) {
  return (
    <aside
      ref={resize.setPaneRef}
      aria-label="Outline"
      className="relative flex h-full shrink-0 flex-col border-l border-stroke bg-background-base"
    >
      <div
        role="separator"
        aria-orientation="vertical"
        aria-label="Resize outline"
        className={`absolute inset-y-0 -left-px z-10 w-1.5 cursor-col-resize touch-none ${
          resize.dragging ? "bg-content/15" : "hover:bg-content/10"
        }`}
        onPointerDown={resize.onPointerDown}
        onDoubleClick={resize.onDoubleClick}
      />
      <OutlineHeader
        onDetach={onDetach}
        onClose={onClose}
        detachLabel="Detach outline"
        detachIcon={AppWindow}
      />
      <OutlineList
        items={items}
        activeId={activeId}
        onSelect={onSelect}
        className="min-h-0 flex-1 overflow-auto py-1"
      />
    </aside>
  );
}

type FloatingProps = {
  items: OutlineItem[];
  activeId: string | null;
  onSelect: (item: OutlineItem) => void;
  onClose: () => void;
  onDock: () => void;
  initialPosition: { x: number; y: number } | null;
  onCommitPosition: (position: { x: number; y: number }) => void;
};

/** The detached outline: a draggable card floating over the workspace. */
export function FloatingOutline({
  items,
  activeId,
  onSelect,
  onClose,
  onDock,
  initialPosition,
  onCommitPosition,
}: FloatingProps) {
  const cardRef = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState(() =>
    clampPosition(
      initialPosition?.x ?? defaultPosition().x,
      initialPosition?.y ?? defaultPosition().y,
      null,
    ),
  );
  const positionRef = useRef(position);
  positionRef.current = position;

  useEffect(() => {
    const onResize = () =>
      setPosition((current) =>
        clampPosition(current.x, current.y, cardRef.current),
      );
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);

  const onHeaderPointerDown = (event: ReactPointerEvent<HTMLElement>) => {
    if (event.button !== 0) return;
    if ((event.target as Element).closest("button")) return;
    event.preventDefault();
    const handle = event.currentTarget;
    const pointerId = event.pointerId;
    const startX = event.clientX;
    const startY = event.clientY;
    const start = positionRef.current;
    handle.setPointerCapture(pointerId);
    const restoreSelection = suppressTextSelection();

    const onMove = (ev: PointerEvent) => {
      if (ev.pointerId !== pointerId) return;
      setPosition(
        clampPosition(
          start.x + (ev.clientX - startX),
          start.y + (ev.clientY - startY),
          cardRef.current,
        ),
      );
    };

    const stop = () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", stop);
      window.removeEventListener("pointercancel", stop);
      restoreSelection();
      try {
        handle.releasePointerCapture(pointerId);
      } catch {
        /* already released */
      }
      onCommitPosition(positionRef.current);
    };

    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", stop);
    window.addEventListener("pointercancel", stop);
  };

  return (
    <div
      ref={cardRef}
      role="dialog"
      aria-label="Outline"
      style={{
        position: "fixed",
        left: position.x,
        top: position.y,
        width: FLOAT_WIDTH,
        zIndex: LAYER.popover,
      }}
      className="isolate flex max-h-[70vh] flex-col overflow-hidden rounded-xl border border-content/10 bg-background-base/85 shadow-xl"
    >
      <GlassBackdrop />
      <div
        onPointerDown={onHeaderPointerDown}
        className="relative z-[1] flex h-8 shrink-0 cursor-grab touch-none items-center gap-1 border-b border-stroke px-1.5 active:cursor-grabbing"
      >
        <GripVertical className="size-3.5 shrink-0 text-content/30" />
        <span className="flex-1 truncate font-sans text-[11px] font-medium text-content/70">
          Outline
        </span>
        <OutlineHeaderActions
          onDetach={onDock}
          onClose={onClose}
          detachLabel="Dock outline"
          detachIcon={PanelRight}
        />
      </div>
      <OutlineList
        items={items}
        activeId={activeId}
        onSelect={onSelect}
        className="relative z-[1] min-h-0 flex-1 overflow-auto py-1"
      />
    </div>
  );
}

function OutlineHeader({
  onDetach,
  onClose,
  detachLabel,
  detachIcon,
}: {
  onDetach: () => void;
  onClose: () => void;
  detachLabel: string;
  detachIcon: IconComponent;
}) {
  return (
    <header className="flex h-8 shrink-0 items-center gap-1 border-b border-stroke pl-2.5 pr-1.5">
      <span className="flex-1 truncate font-sans text-[11px] font-medium text-content/70">
        Outline
      </span>
      <OutlineHeaderActions
        onDetach={onDetach}
        onClose={onClose}
        detachLabel={detachLabel}
        detachIcon={detachIcon}
      />
    </header>
  );
}

function OutlineHeaderActions({
  onDetach,
  onClose,
  detachLabel,
  detachIcon: DetachIcon,
}: {
  onDetach: () => void;
  onClose: () => void;
  detachLabel: string;
  detachIcon: IconComponent;
}) {
  return (
    <div className="flex items-center gap-0.5">
      <button
        type="button"
        title={detachLabel}
        aria-label={detachLabel}
        onClick={onDetach}
        className="grid size-6 place-items-center rounded text-content/55 hover:bg-content/10 hover:text-content"
      >
        <DetachIcon className="size-3.5" strokeWidth={1.75} />
      </button>
      <button
        type="button"
        title="Close outline"
        aria-label="Close outline"
        onClick={onClose}
        className="grid size-6 place-items-center rounded text-content/55 hover:bg-content/10 hover:text-content"
      >
        <X className="size-3.5" strokeWidth={1.75} />
      </button>
    </div>
  );
}

function OutlineList({
  items,
  activeId,
  onSelect,
  className,
}: {
  items: OutlineItem[];
  activeId: string | null;
  onSelect: (item: OutlineItem) => void;
  className?: string;
}) {
  if (items.length === 0) {
    return (
      <p
        className={`px-3 py-3 text-[11px] leading-5 text-content/40 ${className ?? ""}`}
      >
        No symbols in this file.
      </p>
    );
  }
  return (
    <div role="tree" aria-label="Document symbols" className={className}>
      {items.map((item) => {
        const Icon = KIND_ICONS[item.kind];
        const active = item.id === activeId;
        return (
          <button
            key={item.id}
            type="button"
            role="treeitem"
            aria-level={item.depth + 1}
            aria-selected={active}
            title={`${item.label} · line ${item.line}`}
            onClick={() => onSelect(item)}
            style={{ paddingLeft: 8 + item.depth * 12 }}
            className={`flex w-full items-center gap-1.5 py-[3px] pr-2 text-left text-[12px] leading-5 ${
              active
                ? "bg-content/10 text-content"
                : "text-content/65 hover:bg-content/5 hover:text-content"
            }`}
          >
            <Icon
              className={`size-3.5 shrink-0 ${KIND_TINTS[item.kind]}`}
              strokeWidth={1.75}
            />
            <span className="min-w-0 truncate">{item.label}</span>
          </button>
        );
      })}
    </div>
  );
}

function defaultPosition() {
  if (typeof window === "undefined") return { x: 24, y: 96 };
  return {
    x: Math.max(FLOAT_MARGIN, window.innerWidth - FLOAT_WIDTH - 24),
    y: 96,
  };
}

function clampPosition(
  x: number,
  y: number,
  el: HTMLElement | null,
): { x: number; y: number } {
  if (typeof window === "undefined") return { x, y };
  const width = el?.offsetWidth || FLOAT_WIDTH;
  const height = el?.offsetHeight || 320;
  const maxX = Math.max(FLOAT_MARGIN, window.innerWidth - width - FLOAT_MARGIN);
  const maxY = Math.max(
    FLOAT_MARGIN,
    window.innerHeight - height - FLOAT_MARGIN,
  );
  return {
    x: Math.min(Math.max(FLOAT_MARGIN, x), maxX),
    y: Math.min(Math.max(FLOAT_MARGIN, y), maxY),
  };
}
