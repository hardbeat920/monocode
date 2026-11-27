import { useLayoutEffect, useRef, useSyncExternalStore } from "react";
import { createPortal } from "react-dom";
import {
  INSET_LAYOUT_DEFAULT,
  loadInsetLayout,
  saveInsetLayout,
  subscribeInsetLayout,
} from "../../features/settings/model/appearance";
import {
  saveNewLayoutPromptDismissed,
  useNewLayoutPromptDismissed,
} from "../../features/settings/model/displayPrefs";
import { LAYER } from "../../shared/lib/layers";
import { GlassBackdrop } from "./GlassBackdrop";

export function NewLayoutNotice({
  topOffset = 12,
  onHeightChange,
}: {
  topOffset?: number;
  onHeightChange?: (height: number) => void;
}) {
  const panelRef = useRef<HTMLElement>(null);
  const dismissed = useNewLayoutPromptDismissed();
  const enabled = useSyncExternalStore(
    subscribeInsetLayout,
    loadInsetLayout,
    () => INSET_LAYOUT_DEFAULT,
  );
  const visible = !dismissed && !enabled;

  useLayoutEffect(() => {
    const panel = panelRef.current;
    if (!panel || !onHeightChange) return;
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
      aria-label="Try the Focused layout"
      role="status"
      style={{ zIndex: LAYER.toast, top: topOffset }}
      className="fixed right-3 isolate w-[min(340px,calc(100vw-24px))] overflow-hidden rounded-xl border border-content/10 text-content shadow-xl"
    >
      <GlassBackdrop />
      <div className="relative z-[1] px-3 py-3">
        <p className="text-[13px] font-semibold">Try the Focused layout</p>
        <p className="mt-1 text-[12px] leading-relaxed text-content/60">
          A project switcher, workspace tabs and a right panel. Switch back to
          Classic anytime in Settings → Appearance → Layout.
        </p>
        <div className="mt-3 flex justify-end gap-2">
          <button
            type="button"
            className="rounded-md px-2.5 py-1.5 text-[12px] text-content/60 hover:bg-content/10 hover:text-content"
            onClick={() => saveNewLayoutPromptDismissed(true)}
          >
            Not now
          </button>
          <button
            type="button"
            className="rounded-md bg-content/10 px-2.5 py-1.5 text-[12px] font-medium hover:bg-content/15"
            onClick={() => {
              saveNewLayoutPromptDismissed(true);
              saveInsetLayout(true);
            }}
          >
            Try it
          </button>
        </div>
      </div>
    </section>,
    document.body,
  );
}
