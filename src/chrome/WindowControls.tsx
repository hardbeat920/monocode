import { getCurrentWindow } from "@tauri-apps/api/window";
import { Copy, Minus, Square, X } from "./icons";
import { useEffect, useState } from "react";
import { t } from "../lib/i18n";

export function WindowControls() {
  const [isMaximized, setIsMaximized] = useState(false);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let mounted = true;

    const win = getCurrentWindow();
    void win.isMaximized().then((max) => {
      if (mounted) setIsMaximized(max);
    }).catch(() => {});

    void win.onResized(async () => {
      try {
        const max = await win.isMaximized();
        if (mounted) setIsMaximized(max);
      } catch {}
    }).then((unlistenFn) => {
      if (mounted) {
        unlisten = unlistenFn;
      } else {
        unlistenFn();
      }
    }).catch(() => {});

    return () => {
      mounted = false;
      unlisten?.();
    };
  }, []);

  const handleMinimize = () => {
    try {
      void getCurrentWindow().minimize();
    } catch {}
  };

  const handleToggleMaximize = () => {
    try {
      void getCurrentWindow().toggleMaximize();
    } catch {}
  };

  const handleClose = () => {
    try {
      void getCurrentWindow().close();
    } catch {}
  };

  return (
    <div
      className="flex h-full shrink-0 items-stretch border-l border-content/10"
      data-tauri-drag-region="false"
    >
      <button
        type="button"
        title={t("Minimize")}
        aria-label={t("Minimize window")}
        data-tauri-drag-region="false"
        onClick={handleMinimize}
        className="flex w-10 items-center justify-center text-content/60 transition-colors hover:bg-content/10 hover:text-content"
      >
        <Minus className="size-3.5" strokeWidth={1.75} />
      </button>
      <button
        type="button"
        title={isMaximized ? t("Restore") : t("Maximize")}
        aria-label={isMaximized ? t("Restore window") : t("Maximize window")}
        data-tauri-drag-region="false"
        onClick={handleToggleMaximize}
        className="flex w-10 items-center justify-center text-content/60 transition-colors hover:bg-content/10 hover:text-content"
      >
        {isMaximized ? (
          <Copy className="size-3" strokeWidth={1.75} />
        ) : (
          <Square className="size-3" strokeWidth={1.75} />
        )}
      </button>
      <button
        type="button"
        title={t("Close")}
        aria-label={t("Close window")}
        data-tauri-drag-region="false"
        onClick={handleClose}
        className="flex w-10 items-center justify-center text-content/60 transition-colors hover:bg-red-600 hover:text-white"
      >
        <X className="size-3.5" strokeWidth={1.75} />
      </button>
    </div>
  );
}
