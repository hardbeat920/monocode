import type { CSSProperties, ReactNode } from "react";

/** A small rounded status pill; `className` or `style` sets its tint. */
export function Badge({
  className = "",
  style,
  children,
}: {
  className?: string;
  style?: CSSProperties;
  children: ReactNode;
}) {
  return (
    <span
      style={style}
      className={`inline-flex h-5 max-w-full items-center gap-1 rounded-full px-2 text-[11px] font-medium ${className}`}
    >
      {children}
    </span>
  );
}
