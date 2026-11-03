import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useState } from "react";
import { IS_MAC } from "./platform";

/** Native macOS buttons need room only while the window is not fullscreen. */
export function useTrafficLightsVisible(): boolean {
  const [visible, setVisible] = useState(IS_MAC);

  useEffect(() => {
    if (!IS_MAC) return;

    let mounted = true;
    let latest = 0;
    let unlisten: (() => void) | undefined;
    try {
      const win = getCurrentWindow();
      const sync = async () => {
        const request = ++latest;
        try {
          const fullscreen = await win.isFullscreen();
          if (mounted && request === latest) setVisible(!fullscreen);
        } catch {}
      };

      void sync();
      // Native fullscreen transitions emit a resize after updating the state.
      void win
        .onResized(sync)
        .then((stop) => {
          if (!mounted) {
            stop();
            return;
          }
          unlisten = stop;
          // Cover a transition while the listener was being registered.
          void sync();
        })
        .catch(() => {});
    } catch {}

    return () => {
      mounted = false;
      unlisten?.();
    };
  }, []);

  return visible;
}
