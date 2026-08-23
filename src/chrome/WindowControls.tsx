import { getCurrentWindow } from "@tauri-apps/api/window";
import { Copy, Minus, Square, X } from "lucide-react";
import { useEffect, useState } from "react";

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
        title="Minimize"
        aria-label="Minimize window"
        data-tauri-drag-region="false"
        onClick={handleMinimize}
        className="flex w-10 items-center justify-center text-content/60 transition-colors hover:bg-content/10 hover:text-content"
      >
        <Minus className="size-3.5" strokeWidth={1.75} />
      </button>
      <button
        type="button"
        title={isMaximized ? "Restore" : "Maximize"}
        aria-label={isMaximized ? "Restore window" : "Maximize window"}
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
        title="Close"
        aria-label="Close window"
        data-tauri-drag-region="false"
        onClick={handleClose}
        className="flex w-10 items-center justify-center text-content/60 transition-colors hover:bg-red-600 hover:text-white"
      >
        <X className="size-3.5" strokeWidth={1.75} />
      </button>
    </div>
  );
}
