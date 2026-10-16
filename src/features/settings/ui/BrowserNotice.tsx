import { useEffect } from "react";
import { createPortal } from "react-dom";
import { LAYER } from "../../../shared/lib/layers";
import { X } from "../../../shared/ui/icons";
import { SecondaryButton } from "../../../shared/ui/SecondaryButton";
import { dismissBrowserNotice, useBrowserNotice } from "../model/browser";

const NOTICE_MS = 15_000;

/** The browser module's toast (a dropped link, the switch off), with a way
 * to Settings. */
export function BrowserNotice({
  onOpenSettings,
}: {
  onOpenSettings: () => void;
}) {
  const notice = useBrowserNotice();
  useEffect(() => {
    if (!notice) return;
    const timer = window.setTimeout(
      () => dismissBrowserNotice(notice.id),
      NOTICE_MS,
    );
    return () => window.clearTimeout(timer);
  }, [notice]);
  if (!notice) return null;
  return createPortal(
    <div
      role="status"
      aria-live="polite"
      style={{ zIndex: LAYER.toast }}
      className="fixed right-4 bottom-4 flex w-[min(380px,calc(100vw-32px))] items-start gap-3 rounded-xl border border-content/15 bg-background-base/90 px-3 py-2.5 backdrop-blur-xl text-[12px] text-content shadow-xl"
    >
      <span className="min-w-0 flex-1 leading-relaxed">{notice.message}</span>
      {notice.settings ? (
        <SecondaryButton
          onClick={() => {
            dismissBrowserNotice(notice.id);
            onOpenSettings();
          }}
        >
          Open Settings
        </SecondaryButton>
      ) : null}
      <button
        type="button"
        aria-label="Dismiss"
        onClick={() => dismissBrowserNotice(notice.id)}
        className="grid size-6 shrink-0 place-items-center rounded-md text-content/45 hover:bg-content/10 hover:text-content"
      >
        <X className="size-3.5" strokeWidth={1.75} />
      </button>
    </div>,
    document.body,
  );
}
