import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useState } from "react";
import { IS_MAC } from "../../platform/tauri/platform";

/** Whether the macOS traffic lights are drawn over the top-left corner. Other
 * platforms never have them, and macOS hides them in full screen. */
export function useTrafficLights(): boolean {
  const [fullscreen, setFullscreen] = useState(false);

  useEffect(() => {
    if (!IS_MAC) return;
    let mounted = true;
    let unlisten: (() => void) | undefined;
    const sync = (win: ReturnType<typeof getCurrentWindow>) =>
      win
        .isFullscreen()
        .then((next) => {
          if (mounted) setFullscreen(next);
        })
        .catch(() => {});
    try {
      const win = getCurrentWindow();
      void sync(win);
      // Entering or leaving full screen resizes the window.
      void win
        .onResized(() => void sync(win))
        .then((unlistenFn) => {
          if (mounted) unlisten = unlistenFn;
          else unlistenFn();
        })
        .catch(() => {});
    } catch {}
    return () => {
      mounted = false;
      unlisten?.();
    };
  }, []);

  return IS_MAC && !fullscreen;
}
