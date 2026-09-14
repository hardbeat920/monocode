import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import type { ToolPreview } from "../lib/session";
import { FilePreview } from "./FilePreview";
import { Popover } from "./Popover";
import { X } from "./icons";

type Props = {
  preview: ToolPreview;
  label: string;
  status: "pending" | "accepted" | "rejected";
  cwd?: string;
  onOpen?: () => void;
  onOpenFile?: (path: string) => void;
  className?: string;
  children: ReactNode;
};

/** A quiet file chip that reveals the tool's own edit, independent of git. */
export function ToolDiffPreview({
  preview,
  label,
  status,
  cwd,
  onOpen,
  onOpenFile,
  className,
  children,
}: Props) {
  const id = useId();
  const trigger = useRef<HTMLButtonElement>(null);
  const surface = useRef<HTMLDivElement>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const hovered = useRef(false);
  const [open, setOpen] = useState(false);

  function clearTimer() {
    clearTimeout(timer.current);
  }

  useEffect(() => () => clearTimeout(timer.current), []);

  function show() {
    clearTimer();
    setOpen(true);
  }

  function dismiss(restoreFocus = false) {
    clearTimer();
    // Restore before closing so focus does not reopen the preview.
    if (restoreFocus && surface.current?.contains(document.activeElement)) {
      trigger.current?.focus();
    }
    hovered.current = false;
    setOpen(false);
  }

  function scheduleClose() {
    clearTimer();
    timer.current = setTimeout(() => {
      if (
        !hovered.current &&
        document.activeElement !== trigger.current &&
        !surface.current?.contains(document.activeElement)
      ) {
        setOpen(false);
      }
    }, 180);
  }

  const description =
    status === "pending"
      ? "Proposed changes"
      : status === "rejected"
        ? "Attempted changes · tool did not complete"
        : preview.contentOnly
          ? "Written content · previous contents unavailable"
          : "Change preview";

  return (
    <>
      <button
        ref={trigger}
        type="button"
        className={`${className ?? ""} focus-visible:outline-2 focus-visible:outline-sky-400/60`}
        aria-label={
          onOpen
            ? `Open ${label}`
            : `Preview ${preview.contentOnly ? "written content" : "changes"}: ${label}`
        }
        aria-haspopup="dialog"
        aria-expanded={open}
        aria-controls={open ? id : undefined}
        onPointerEnter={(event) => {
          if (event.pointerType === "touch") return;
          hovered.current = true;
          clearTimer();
          timer.current = setTimeout(() => setOpen(true), 300);
        }}
        onPointerLeave={() => {
          hovered.current = false;
          scheduleClose();
        }}
        onFocus={show}
        onBlur={scheduleClose}
        onClick={(event) => {
          event.stopPropagation();
          dismiss();
          onOpen?.();
        }}
        onKeyDown={(event) => {
          if (event.key === "ArrowDown" && open) {
            event.preventDefault();
            surface.current?.focus();
          } else if (event.key === "Tab" && !event.shiftKey && open) {
            event.preventDefault();
            surface.current
              ?.querySelector<HTMLElement>("button, [tabindex='0']")
              ?.focus();
          }
        }}
      >
        {children}
      </button>
      {open ? (
        <Popover
          ref={surface}
          anchor={trigger}
          id={id}
          role="dialog"
          aria-label={`${description}: ${label}`}
          tabIndex={-1}
          width={460}
          maxHeight={280}
          className="overflow-auto overscroll-contain"
          onDismiss={(reason) => dismiss(reason === "escape")}
          onPointerEnter={() => {
            hovered.current = true;
            clearTimer();
          }}
          onPointerLeave={() => {
            hovered.current = false;
            scheduleClose();
          }}
          onFocus={show}
          onBlur={scheduleClose}
          onKeyDown={(event) => {
            if (event.key !== "Tab" || !event.shiftKey) return;
            const first = surface.current?.querySelector(
              "button, [tabindex='0']",
            );
            if (event.target === first || event.target === surface.current) {
              event.preventDefault();
              trigger.current?.focus();
            }
          }}
        >
          <div className="flex items-center gap-2 border-b border-content/8 px-2.5 py-1.5 font-sans text-[11px] text-content/50">
            <span className="min-w-0 flex-1">{description}</span>
            <button
              type="button"
              aria-label="Close preview"
              className="shrink-0 rounded p-0.5 hover:bg-content/8 hover:text-content"
              onClick={() => dismiss(true)}
            >
              <X className="size-3" />
            </button>
          </div>
          <FilePreview
            preview={preview}
            status={status}
            cwd={cwd}
            variant="popover"
            onOpenFile={
              onOpenFile
                ? (path) => {
                    dismiss();
                    onOpenFile(path);
                  }
                : undefined
            }
          />
        </Popover>
      ) : null}
    </>
  );
}
