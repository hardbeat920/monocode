import { forwardRef, type ComponentPropsWithoutRef, type ReactNode } from "react";
import { ChevronDown, ChevronRight } from "./icons";

/** Label colour for a path's git status, as the explorer shows it. */
export const GIT_STATUS_COLOR: Record<string, string> = {
  modified: "text-amber-400",
  added: "text-diff-add-fg",
  untracked: "text-diff-add-fg",
  deleted: "text-diff-del-fg",
};

type Props = Omit<ComponentPropsWithoutRef<"button">, "style"> & {
  depth: number;
  selected: boolean;
  /** Folder state; `undefined` leaves the chevron slot empty. */
  open?: boolean;
  icon: ReactNode;
};

/** One explorer-style tree row: indent, chevron, icon, then the label as children. */
export const TreeRow = forwardRef<HTMLButtonElement, Props>(function TreeRow(
  { depth, selected, open, icon, className = "", children, ...props },
  ref,
) {
  return (
    <button
      ref={ref}
      type="button"
      role="treeitem"
      {...props}
      style={{ paddingLeft: 8 + depth * 12 }}
      className={`flex h-7.5 w-full cursor-default items-center gap-1 pr-2 text-left text-[14px] outline-none focus-visible:outline-1 focus-visible:-outline-offset-1 focus-visible:outline-accent/40 leading-none ${
        selected
          ? "bg-selection text-content"
          : "text-content hover:bg-content/5"
      } ${className}`}
    >
      <span className="grid size-4 shrink-0 place-items-center text-content/50">
        {open === undefined ? null : open ? (
          <ChevronDown className="size-3.5" strokeWidth={1.75} />
        ) : (
          <ChevronRight className="size-3.5" strokeWidth={1.75} />
        )}
      </span>
      <span className="shrink-0">{icon}</span>
      {children}
    </button>
  );
});
