import { useEffect, useState, type ReactNode } from "react";

/**
 * Grows its contents open from nothing, pushing what follows down, and folds
 * them away again before unmounting. Opening later always grows; on mount it
 * grows only with `animateIn`, so content that is simply there on load does
 * not animate.
 */
export function RevealHeight({
  open = true,
  animateIn = false,
  className = "",
  children,
  ...attributes
}: {
  open?: boolean;
  animateIn?: boolean;
  className?: string;
  children: ReactNode;
} & Record<`data-${string}`, string | undefined>) {
  const [growIn, setGrowIn] = useState(animateIn);
  const [wasOpen, setWasOpen] = useState(open);
  // Settled during render, so the first frame of a reopen already grows.
  if (open !== wasOpen) {
    setWasOpen(open);
    if (open) setGrowIn(true);
  }
  const [rendered, setRendered] = useState(open);
  const closing = rendered && !open;

  useEffect(() => {
    if (open) {
      setRendered(true);
      return;
    }
    // Without motion there is no animation to end, so leave at once.
    if (window.matchMedia?.("(prefers-reduced-motion: reduce)").matches)
      setRendered(false);
  }, [open]);

  if (!rendered && !open) return null;
  return (
    <div
      {...attributes}
      className={`${className} ${
        closing ? "reveal-height-out" : growIn ? "reveal-height-in" : ""
      }`}
      inert={closing || undefined}
      onAnimationEnd={(event) => {
        if (event.target === event.currentTarget && closing) setRendered(false);
      }}
    >
      {children}
    </div>
  );
}
