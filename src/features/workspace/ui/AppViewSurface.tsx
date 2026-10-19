import type { ReactNode } from "react";

/** Keep an open app tab's draft and selection while another tab is active. */
export function AppViewSurface({
  active,
  children,
}: {
  active: boolean;
  children: ReactNode;
}) {
  return (
    <div
      className={active ? "flex min-h-0 min-w-0 flex-1 flex-col" : "hidden"}
      aria-hidden={!active}
      inert={!active || undefined}
    >
      {children}
    </div>
  );
}
