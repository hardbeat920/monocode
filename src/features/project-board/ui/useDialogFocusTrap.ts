import { useEffect, useRef } from "react";

export interface ActiveDialogEntry {
  id: string;
  element: HTMLElement;
  onClose: () => void;
}

const activeDialogStack: ActiveDialogEntry[] = [];

export function getActiveDialogStack(): readonly ActiveDialogEntry[] {
  return activeDialogStack;
}

export function clearActiveDialogStack(): void {
  activeDialogStack.length = 0;
}

export function isTopmostDialog(element: HTMLElement | null): boolean {
  if (!element || activeDialogStack.length === 0) return false;
  return activeDialogStack[activeDialogStack.length - 1].element === element;
}

export interface UseDialogFocusTrapOptions {
  id?: string;
  onClose: () => void;
  initialFocusRef?: React.RefObject<HTMLElement | null>;
  disabled?: boolean;
}

export function getFocusableElements(container: HTMLElement): HTMLElement[] {
  const selector = [
    "a[href]",
    "button:not([disabled])",
    "textarea:not([disabled])",
    "input:not([disabled])",
    "select:not([disabled])",
    '[tabindex]:not([tabindex="-1"])',
  ].join(", ");

  const elements = Array.from(container.querySelectorAll<HTMLElement>(selector));
  return elements.filter((el) => {
    if (el.getAttribute("aria-hidden") === "true") return false;
    if (el instanceof HTMLInputElement && el.type === "hidden") return false;
    if (el.classList.contains("hidden")) return false;
    if (el.style.display === "none" || el.style.visibility === "hidden") return false;
    return true;
  });
}

export function useDialogFocusTrap(
  containerRef: React.RefObject<HTMLElement | null>,
  options: UseDialogFocusTrapOptions,
) {
  const { onClose, initialFocusRef, disabled = false } = options;
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;
  const openerRef = useRef<HTMLElement | null>(null);
  const idRef = useRef(options.id || Math.random().toString(36).slice(2));

  useEffect(() => {
    if (disabled) return;

    if (document.activeElement instanceof HTMLElement) {
      openerRef.current = document.activeElement;
    }

    const container = containerRef.current;
    if (!container) return;

    const entry: ActiveDialogEntry = {
      id: idRef.current,
      element: container,
      onClose: () => onCloseRef.current(),
    };
    activeDialogStack.push(entry);

    if (initialFocusRef?.current) {
      initialFocusRef.current.focus();
    } else {
      const focusables = getFocusableElements(container);
      if (focusables.length > 0) {
        focusables[0].focus();
      } else {
        container.focus?.();
      }
    }

    const handleKeyDown = (e: KeyboardEvent) => {
      const isTopmost =
        activeDialogStack.length > 0 &&
        activeDialogStack[activeDialogStack.length - 1].element === container;

      if (!isTopmost) return;

      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        onCloseRef.current();
        return;
      }

      if (e.key === "Tab") {
        const focusables = getFocusableElements(container);
        if (focusables.length === 0) {
          e.preventDefault();
          return;
        }

        const first = focusables[0];
        const last = focusables[focusables.length - 1];
        const currentActive = document.activeElement;

        if (!container.contains(currentActive)) {
          e.preventDefault();
          if (e.shiftKey) {
            last.focus();
          } else {
            first.focus();
          }
          return;
        }

        if (e.shiftKey) {
          if (currentActive === first) {
            e.preventDefault();
            last.focus();
          }
        } else {
          if (currentActive === last) {
            e.preventDefault();
            first.focus();
          }
        }
      }
    };

    window.addEventListener("keydown", handleKeyDown, true);

    return () => {
      window.removeEventListener("keydown", handleKeyDown, true);

      const idx = activeDialogStack.findIndex((e) => e.id === entry.id);
      if (idx !== -1) {
        activeDialogStack.splice(idx, 1);
      }

      if (
        openerRef.current &&
        typeof openerRef.current.focus === "function" &&
        document.contains(openerRef.current)
      ) {
        openerRef.current.focus();
      }
    };
  }, [containerRef, disabled, initialFocusRef]);
}
