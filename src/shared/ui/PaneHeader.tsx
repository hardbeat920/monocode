import type { ComponentProps } from "react";

/** Shared toolbar sizing for the workspace's project panes. */
export function PaneHeader({
  compact = false,
  className = "",
  ...props
}: ComponentProps<"div"> & { compact?: boolean }) {
  return (
    <div
      {...props}
      className={`flex min-h-0 min-w-0 shrink-0 items-center gap-2 border-b border-stroke px-3 text-[12px] font-medium text-content/85 ${compact ? "h-8" : "h-9"} ${className}`}
    />
  );
}
