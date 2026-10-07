import {
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  ChevronUp,
  PanelBottom,
  PanelLeft,
  PanelRight,
  PanelTop,
  Plus,
} from "../../../shared/ui/icons";
import {
  useEffect,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
} from "react";
import { ExplorerMenu } from "../../files/ui/ExplorerMenu";
import { SurfaceTabs } from "./SurfaceTabs";
import { IconButton } from "../../../app/shell/TitleBar";
import {
  clampDockSize,
  isVerticalDock,
  type DockSide,
  type ProjectTerminalDock,
} from "../../projects/model/projectTerminal";

type Props = {
  dock: ProjectTerminalDock;
  /** Singular surface name: "Terminal", "Browser". */
  name: string;
  /** Accessible name of the tab strip. */
  tabsLabel: string;
  addLabel: string;
  hideLabel: string;
  /** Size a double-click on the sash restores. */
  defaultSize: (side: DockSide) => number;
  /**
   * Keep the whole sash outside the dock. Native content (a browser page)
   * covers the DOM, so a sash that overlaps it could not be grabbed there.
   */
  sashOutside?: boolean;
  onFocus?: () => void;
  onHide: () => void;
  onSideChange: (side: DockSide) => void;
  onSizePaint: (size: number) => void;
  onSizeCommit: (size: number) => void;
  /** Called with the sash state so native content can stay out of the way. */
  onResizingChange?: (resizing: boolean) => void;
  onAdd: () => void;
  onSelect: (fileId: string) => void;
  onClose: (fileId: string) => void;
  onCloseOthers: (fileId: string) => void;
  onReorder: (ids: string[]) => void;
  children: ReactNode;
};

const SIDE_ITEMS: { id: DockSide; label: string }[] = [
  { id: "bottom", label: "Dock Bottom" },
  { id: "top", label: "Dock Top" },
  { id: "left", label: "Dock Left" },
  { id: "right", label: "Dock Right" },
];

function sideIcon(side: DockSide) {
  if (side === "top") return PanelTop;
  if (side === "left") return PanelLeft;
  if (side === "right") return PanelRight;
  return PanelBottom;
}

function hideIcon(side: DockSide) {
  if (side === "top") return ChevronUp;
  if (side === "left") return ChevronLeft;
  if (side === "right") return ChevronRight;
  return ChevronDown;
}

function sashClass(side: DockSide, outside: boolean): string {
  const base = "absolute z-10 touch-none";
  if (side === "top") {
    return `${base} inset-x-0 h-1.5 cursor-row-resize ${outside ? "-bottom-1.5" : "-bottom-px"}`;
  }
  if (side === "bottom") {
    return `${base} inset-x-0 h-1.5 cursor-row-resize ${outside ? "-top-1.5" : "-top-px"}`;
  }
  if (side === "left") {
    return `${base} inset-y-0 w-1.5 cursor-col-resize ${outside ? "-right-1.5" : "-right-px"}`;
  }
  return `${base} inset-y-0 w-1.5 cursor-col-resize ${outside ? "-left-1.5" : "-left-px"}`;
}

/** Resizable, movable dock with a tab strip: the frame shared by every dock. */
export function DockPanel({
  dock,
  name,
  tabsLabel,
  addLabel,
  hideLabel,
  defaultSize,
  sashOutside = false,
  onFocus,
  onHide,
  onSideChange,
  onSizePaint,
  onSizeCommit,
  onResizingChange,
  onAdd,
  onSelect,
  onClose,
  onCloseOthers,
  onReorder,
  children,
}: Props) {
  const vertical = isVerticalDock(dock.side);
  const [dragging, setDragging] = useState(false);
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const sideButton = useRef<HTMLDivElement>(null);
  const drag = useRef<{ start: number; size: number } | null>(null);
  const sizeRef = useRef(dock.size);
  sizeRef.current = dock.size;
  const pending = useRef(dock.size);
  const frame = useRef<number | null>(null);
  const SideIcon = sideIcon(dock.side);
  const HideIcon = hideIcon(dock.side);

  useEffect(() => {
    if (!dragging) return;
    const previous = document.body.style.cursor;
    document.body.style.cursor = vertical ? "row-resize" : "col-resize";
    return () => {
      document.body.style.cursor = previous;
    };
  }, [dragging, vertical]);

  useEffect(() => {
    onResizingChange?.(dragging);
  }, [dragging, onResizingChange]);

  useEffect(
    () => () => {
      if (frame.current != null) cancelAnimationFrame(frame.current);
    },
    [],
  );

  const viewport = () => ({
    width: window.innerWidth,
    height: window.innerHeight,
  });

  const paint = (next: number) => {
    pending.current = next;
    if (frame.current != null) return;
    frame.current = requestAnimationFrame(() => {
      frame.current = null;
      onSizePaint(pending.current);
    });
  };

  const commit = () => {
    if (frame.current != null) {
      cancelAnimationFrame(frame.current);
      frame.current = null;
    }
    onSizeCommit(pending.current);
  };

  const onResizePointerDown = (event: ReactPointerEvent<HTMLDivElement>) => {
    event.preventDefault();
    event.stopPropagation();
    event.currentTarget.setPointerCapture(event.pointerId);
    drag.current = {
      start: vertical ? event.clientY : event.clientX,
      size: sizeRef.current,
    };
    pending.current = sizeRef.current;
    setDragging(true);
  };

  const onResizePointerMove = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (!drag.current) return;
    const point = vertical ? event.clientY : event.clientX;
    const delta = point - drag.current.start;
    const signed =
      dock.side === "bottom" || dock.side === "right" ? -delta : delta;
    paint(clampDockSize(dock.side, drag.current.size + signed, viewport()));
  };

  const onResizePointerUp = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (!drag.current) return;
    drag.current = null;
    commit();
    setDragging(false);
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
  };

  return (
    <section
      className={`relative flex h-full min-h-0 min-w-0 flex-col bg-transparent ${
        dock.side === "top"
          ? "border-b"
          : dock.side === "bottom"
            ? "border-t"
            : dock.side === "left"
              ? "border-r"
              : "border-l"
      } border-stroke`}
      onMouseDown={onFocus}
    >
      <div
        role="separator"
        aria-orientation={vertical ? "horizontal" : "vertical"}
        aria-label={`Resize ${name.toLowerCase()}`}
        aria-valuenow={dock.size}
        className={`${sashClass(dock.side, sashOutside)} ${dragging ? "bg-content/15" : "hover:bg-content/10"}`}
        onPointerDown={onResizePointerDown}
        onPointerMove={onResizePointerMove}
        onPointerUp={onResizePointerUp}
        onPointerCancel={onResizePointerUp}
        onDoubleClick={() => {
          pending.current = defaultSize(dock.side);
          commit();
        }}
      />
      <SurfaceTabs
        files={dock.pane.files}
        activeFileId={dock.pane.activeFileId}
        dirtyFileIds={EMPTY_IDS}
        fileErrorCounts={EMPTY_ERRORS}
        label={tabsLabel}
        onSelectFile={onSelect}
        onCloseFile={onClose}
        onCloseOtherFiles={onCloseOthers}
        onReorder={onReorder}
        trailing={
          <div className="flex shrink-0 items-center gap-0.5 pr-1.5">
            <IconButton label={addLabel} onClick={onAdd}>
              <Plus className="size-3.5" strokeWidth={1.75} />
            </IconButton>
            <div ref={sideButton}>
              <IconButton
                label={`Move ${name}`}
                onClick={() => {
                  const rect = sideButton.current?.getBoundingClientRect();
                  if (!rect) return;
                  setMenu({ x: rect.left, y: rect.bottom + 4 });
                }}
              >
                <SideIcon className="size-3.5" strokeWidth={1.75} />
              </IconButton>
            </div>
            <IconButton label={hideLabel} onClick={onHide}>
              <HideIcon className="size-3.5" strokeWidth={1.75} />
            </IconButton>
          </div>
        }
      />
      <div className="relative min-h-0 min-w-0 flex-1">{children}</div>
      {menu ? (
        <ExplorerMenu
          x={menu.x}
          y={menu.y}
          ariaLabel={`Move ${name.toLowerCase()}`}
          items={SIDE_ITEMS.map((item) => ({
            kind: "item" as const,
            id: item.id,
            label: item.label,
            checked: item.id === dock.side,
          }))}
          onPick={(id) => {
            if (
              id === "top" ||
              id === "bottom" ||
              id === "left" ||
              id === "right"
            ) {
              onSideChange(id);
            }
            setMenu(null);
          }}
          onClose={() => setMenu(null)}
        />
      ) : null}
    </section>
  );
}

const EMPTY_IDS = new Set<string>();
const EMPTY_ERRORS = new Map<string, number>();
