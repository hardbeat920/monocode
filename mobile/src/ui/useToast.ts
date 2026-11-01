import { useCallback, useEffect, useRef, useState } from "react";

export type ToastState = {
  id: number;
  message: string;
  /** Present while the change can still be taken back. */
  undo?: () => void;
};

/** How long a toast stays before it goes by itself. */
export const TOAST_MS = 5000;

/**
 * The one line under a list that says what a change did. One at a time; a
 * newer toast replaces the one showing. Shared by the Inbox and Sessions.
 */
export function useToast(ms = TOAST_MS) {
  const [toast, setToast] = useState<ToastState | null>(null);
  const toastId = useRef(0);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  useEffect(() => () => clearTimeout(timer.current), []);
  const dismiss = useCallback(() => {
    clearTimeout(timer.current);
    setToast(null);
  }, []);
  const show = useCallback(
    (message: string, undo?: () => void) => {
      clearTimeout(timer.current);
      const id = ++toastId.current;
      setToast({ id, message, undo });
      timer.current = setTimeout(
        () => setToast((now) => (now?.id === id ? null : now)),
        ms,
      );
    },
    [ms],
  );
  return { toast, show, dismiss };
}
