import { useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { GlassBackdrop } from "../../../app/shell/GlassBackdrop";
import { LAYER } from "../../../shared/lib/layers";
import { X } from "../../../shared/ui/icons";
import { useModHost } from "../runtime/host";
import { toneColor } from "./ModElement";

/** `$.ui.toast` messages from every mod, stacked top right. */
export function ModToasts({
  topOffset = 12,
  onHeightChange,
}: {
  topOffset?: number;
  onHeightChange?: (height: number) => void;
}) {
  const host = useModHost();
  const panelRef = useRef<HTMLElement>(null);
  // The host expires toasts; a dismissal only hides it in this window.
  const [hidden, setHidden] = useState<ReadonlySet<string>>(() => new Set());
  const toasts = host.toasts().filter((toast) => !hidden.has(toast.key));
  const hide = (key: string) =>
    setHidden((current) => new Set(current).add(key));
  const visible = toasts.length > 0;
  useLayoutEffect(() => {
    const panel = panelRef.current;
    if (!panel || !onHeightChange || typeof ResizeObserver === "undefined")
      return;
    const measure = () => onHeightChange(panel.offsetHeight);
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(panel);
    return () => {
      observer.disconnect();
      onHeightChange(0);
    };
  }, [visible, onHeightChange]);
  if (!visible) return null;

  return createPortal(
    <section
      ref={panelRef}
      aria-label="Mod notifications"
      role="status"
      style={{ zIndex: LAYER.toast, top: topOffset }}
      className="fixed right-3 isolate w-[min(340px,calc(100vw-24px))] overflow-hidden rounded-xl border border-content/10 text-content shadow-xl"
    >
      <GlassBackdrop />
      <div className="relative z-[1] divide-y divide-stroke">
        {toasts.map((toast) => (
          <div key={toast.key} className="flex items-start gap-2 px-3 py-2.5">
            <span
              aria-hidden
              className="mt-1.5 size-1.5 shrink-0 rounded-full bg-content/40"
              style={{ backgroundColor: toneColor(toast.tone) }}
            />
            <div className="min-w-0 flex-1">
              <div className="text-[11px] font-medium text-content/45">
                {toast.name}
              </div>
              <p className="whitespace-pre-wrap break-words text-[12px] leading-5">
                {toast.text}
              </p>
            </div>
            <button
              type="button"
              aria-label="Dismiss"
              className="grid size-6 shrink-0 place-items-center rounded-md text-content/40 hover:bg-content/10 hover:text-content"
              onClick={() => hide(toast.key)}
            >
              <X className="size-3" strokeWidth={2} />
            </button>
          </div>
        ))}
      </div>
    </section>,
    document.body,
  );
}
