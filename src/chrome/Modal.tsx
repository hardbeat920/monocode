import { X } from "./icons";
import {
  useEffect,
  useId,
  useRef,
  type KeyboardEvent as ReactKeyboardEvent,
  type ReactNode,
  type RefObject,
} from "react";
import { createPortal } from "react-dom";
import { useLockOverscroll } from "../hooks/useLockOverscroll";
import { LAYER } from "../lib/layers";

export type ModalSize = "sm" | "md";

const WIDTH: Record<ModalSize, string> = {
  sm: "w-[min(420px,calc(100vw-24px))]",
  md: "w-[min(560px,calc(100vw-24px))]",
};

const TOP: Record<ModalSize, string> = {
  sm: "top-[22%]",
  md: "top-[10%]",
};

type Props = {
  onClose: () => void;
  title: string;
  description?: string;
  size?: ModalSize;
  /** Extra classes on the panel (fixed height, etc). */
  className?: string;
  initialFocusRef?: RefObject<HTMLElement | null>;
  closeDisabled?: boolean;
  children: ReactNode;
};

export function ModalPanel({
  onClose,
  title,
  description,
  size = "md",
  className,
  initialFocusRef,
  closeDisabled = false,
  children,
}: Props) {
  const closeRef = useRef<HTMLButtonElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const lockOverscroll = useLockOverscroll<HTMLDivElement>();
  const uid = useId();
  const titleId = `${uid}-title`;
  const descriptionId = description ? `${uid}-desc` : undefined;

  useEffect(() => {
    const previous =
      document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null;
    const initial = initialFocusRef?.current;
    (initial && !initial.matches(":disabled")
      ? initial
      : closeRef.current
    )?.focus();
    return () => {
      if (previous?.isConnected) previous.focus();
    };
  }, [initialFocusRef]);

  useEffect(() => {
    if (closeDisabled) closeRef.current?.focus();
  }, [closeDisabled]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      if (closeDisabled) return;
      onClose();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [closeDisabled, onClose]);

  const onDialogKeyDown = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    if (event.key !== "Tab") return;
    const focusable = panelRef.current?.querySelectorAll<HTMLElement>(
      'button:not(:disabled), input:not(:disabled), textarea:not(:disabled), select:not(:disabled), a[href], [tabindex]:not([tabindex="-1"])',
    );
    if (!focusable?.length) return;
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  };

  return (
    <div
      className={`absolute left-1/2 ${TOP[size]} ${WIDTH[size]} -translate-x-1/2`}
    >
      <div
        ref={panelRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        aria-describedby={descriptionId}
        onKeyDown={onDialogKeyDown}
        onMouseDown={(event) => event.stopPropagation()}
        className={`modal-panel flex flex-col overflow-hidden rounded-2xl border border-content/10 bg-background-base/55 shadow-2xl backdrop-blur-xl ${className ?? ""}`}
      >
        <header className="flex shrink-0 items-start gap-2 px-4 pt-3">
          <div className="min-w-0 flex-1 pt-0.5">
            <h2
              id={titleId}
              className="text-2xl font-semibold leading-tight text-content"
            >
              {title}
            </h2>
            {description ? (
              <p
                id={descriptionId}
                title={description}
                className="mt-0.5 truncate text-[12px] leading-snug text-content/50"
              >
                {description}
              </p>
            ) : null}
          </div>
          <button
            ref={closeRef}
            type="button"
            aria-label="Close"
            aria-disabled={closeDisabled || undefined}
            onClick={closeDisabled ? undefined : onClose}
            className="grid size-7 shrink-0 place-items-center rounded-md text-content/45 hover:bg-content/8 hover:text-content focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent aria-disabled:cursor-default aria-disabled:opacity-30 aria-disabled:hover:bg-transparent aria-disabled:hover:text-content/45"
          >
            <X className="size-3.5" strokeWidth={1.75} />
          </button>
        </header>
        <div
          ref={lockOverscroll}
          className="min-h-0 flex-1 overflow-y-auto overscroll-none"
        >
          {children}
        </div>
      </div>
    </div>
  );
}

export function Modal(props: Props) {
  return createPortal(
    <div className="fixed inset-0" style={{ zIndex: LAYER.dialog }}>
      <div
        className="modal-backdrop absolute inset-0 bg-black/40"
        onMouseDown={props.closeDisabled ? undefined : props.onClose}
      />
      <ModalPanel {...props} />
    </div>,
    document.body,
  );
}
