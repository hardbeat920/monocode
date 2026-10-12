import { useCallback, useEffect, useRef, useState } from "react";

export type HoverCardTiming = {
  /** Delay before a hover opens the card. Focus always opens immediately. */
  openDelayMs?: number;
  /** Grace period after leaving before the card closes, so it can be entered. */
  closeDelayMs?: number;
};

export type HoverCardController = {
  open: boolean;
  /** Open at once, for focus or a deliberate reveal. Cancels pending timers. */
  openNow: () => void;
  /** Arm the open timer; a repeat call before it fires is a no-op. */
  openAfterDelay: () => void;
  /** Arm the close timer. Call from the trigger and the card's own surface. */
  closeAfterDelay: () => void;
  /** Close at once. Cancels pending timers. */
  closeNow: () => void;
  /** Drop a pending close without changing visibility, for pointer re-entry. */
  cancelClose: () => void;
};

/**
 * VS Code's hover timings, from `editorOptions.ts`: `editor.hover.delay`
 * defaults to 300 and `editor.hover.hidingDelay` to 300, with
 * `editor.hover.sticky` on. Both are settings there; here they are the
 * defaults a caller can still override.
 */
const DEFAULT_OPEN_DELAY_MS = 300;
const DEFAULT_CLOSE_DELAY_MS = 300;

/**
 * Hover/focus visibility for an anchored card (tooltips, previews). Timers are
 * refs so re-renders do not re-arm them, and a close is delayed so a pointer
 * can travel from the trigger onto the card without it disappearing.
 */
export function useHoverCard({
  openDelayMs = DEFAULT_OPEN_DELAY_MS,
  closeDelayMs = DEFAULT_CLOSE_DELAY_MS,
}: HoverCardTiming = {}): HoverCardController {
  const [open, setOpen] = useState(false);
  const openRef = useRef(false);
  const openTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const closeTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const setOpenState = useCallback((value: boolean) => {
    openRef.current = value;
    setOpen(value);
  }, []);

  const cancelOpen = useCallback(() => {
    if (openTimer.current == null) return;
    clearTimeout(openTimer.current);
    openTimer.current = null;
  }, []);

  const cancelClose = useCallback(() => {
    if (closeTimer.current == null) return;
    clearTimeout(closeTimer.current);
    closeTimer.current = null;
  }, []);

  useEffect(() => {
    return () => {
      cancelOpen();
      cancelClose();
    };
  }, [cancelOpen, cancelClose]);

  const openNow = useCallback(() => {
    cancelOpen();
    cancelClose();
    setOpenState(true);
  }, [cancelClose, cancelOpen, setOpenState]);

  const openAfterDelay = useCallback(() => {
    cancelClose();
    if (openRef.current || openTimer.current != null) return;
    openTimer.current = setTimeout(() => {
      openTimer.current = null;
      setOpenState(true);
    }, openDelayMs);
  }, [cancelClose, openDelayMs, setOpenState]);

  const closeNow = useCallback(() => {
    cancelOpen();
    cancelClose();
    setOpenState(false);
  }, [cancelClose, cancelOpen, setOpenState]);

  const closeAfterDelay = useCallback(() => {
    cancelOpen();
    cancelClose();
    closeTimer.current = setTimeout(() => {
      closeTimer.current = null;
      setOpenState(false);
    }, closeDelayMs);
  }, [cancelOpen, cancelClose, closeDelayMs, setOpenState]);

  return {
    open,
    openNow,
    openAfterDelay,
    closeAfterDelay,
    closeNow,
    cancelClose,
  };
}
