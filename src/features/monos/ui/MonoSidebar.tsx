import type { CSSProperties, ReactNode } from "react";
import { IconButton } from "../../../app/shell/TitleBar";
import { useDragResize } from "../../../shared/hooks/useDragResize";
import { PanelRightToggle } from "../../../shared/ui/icons";

const MIN_WIDTH = 340;
let rememberedWidth = MIN_WIDTH;
let rememberedArtifactWidth = 560;

/** The shared frame for a Mono's details and turn sidebars. */
export function MonoSidebar({
  open,
  kind,
  label,
  color,
  windowControls,
  children,
}: {
  open: boolean;
  kind: "details" | "activity" | "sessions" | "artifact";
  label: string;
  color: string;
  windowControls?: ReactNode;
  children: ReactNode;
}) {
  const resize = useDragResize({
    min: kind === "artifact" ? 360 : MIN_WIDTH,
    max: () =>
      kind === "artifact"
        ? Math.min(840, Math.round(window.innerWidth * 0.58))
        : Math.min(440, Math.round(window.innerWidth * 0.4)),
    defaultWidth: kind === "artifact" ? 560 : MIN_WIDTH,
    initial: kind === "artifact" ? rememberedArtifactWidth : rememberedWidth,
    direction: "left",
    onCommit: (width) => {
      if (kind === "artifact") rememberedArtifactWidth = width;
      else rememberedWidth = width;
    },
  });
  return (
    <aside
      ref={resize.setPaneRef}
      aria-label={label}
      aria-hidden={!open || undefined}
      inert={!open || undefined}
      data-mono-details={kind === "details" ? "" : undefined}
      data-mono-activity={kind === "activity" ? "" : undefined}
      data-mono-sessions={kind === "sessions" ? "" : undefined}
      data-mono-artifact={kind === "artifact" ? "" : undefined}
      data-open={open}
      style={
        {
          "--mono-color": color,
          "--mono-window-controls-width": windowControls
            ? "calc(7.5rem + 1px)"
            : "0px",
        } as CSSProperties
      }
      className="mono-details-panel relative flex h-full min-h-0 shrink-0 flex-col border-l border-stroke font-sans"
    >
      <div
        role="separator"
        aria-orientation="vertical"
        aria-label={`Resize Mono ${kind}`}
        className={`absolute inset-y-0 -left-1 z-20 w-2 cursor-col-resize touch-none ${resize.dragging ? "bg-content/15" : "hover:bg-content/10"}`}
        onPointerDown={resize.onPointerDown}
        onDoubleClick={resize.onDoubleClick}
      />
      {windowControls ? (
        <div className="absolute right-0 top-0 z-30 h-10">{windowControls}</div>
      ) : null}
      {children}
    </aside>
  );
}

export function MonoSidebarHeader({
  title,
  onClose,
  actions,
}: {
  title: string;
  onClose: () => void;
  actions?: ReactNode;
}) {
  return (
    <header
      className="flex h-10 shrink-0 items-stretch border-b border-stroke"
      style={{ paddingRight: "var(--mono-window-controls-width)" }}
      data-tauri-drag-region="deep"
    >
      <h3 className="flex min-w-0 flex-1 items-center pl-4 text-[13px] font-medium text-content">
        {title}
      </h3>
      <div className="flex shrink-0 items-center gap-0.5 px-3">
        {actions}
        <IconButton
          label={`Hide ${title.toLowerCase()}`}
          active
          onClick={onClose}
        >
          <PanelRightToggle className="size-3.5" strokeWidth={1.75} />
        </IconButton>
      </div>
    </header>
  );
}
