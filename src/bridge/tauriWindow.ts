import { isTauriRuntime } from "./isTauri";
import { getCurrentWindow as nativeGetCurrentWindow } from "tauri-native-window";

function noopUnlisten() {}

function remoteWindow() {
  return {
    label: "remote",
    async isMaximized() {
      return false;
    },
    async minimize() {},
    async toggleMaximize() {},
    async close() {},
    async onResized() {
      return noopUnlisten;
    },
    async onFocusChanged(
      handler: (event: { payload: boolean }) => void,
    ): Promise<() => void> {
      const onFocus = () => handler({ payload: true });
      const onBlur = () => handler({ payload: false });
      window.addEventListener("focus", onFocus);
      window.addEventListener("blur", onBlur);
      return () => {
        window.removeEventListener("focus", onFocus);
        window.removeEventListener("blur", onBlur);
      };
    },
    async onCloseRequested(
      _handler: (event: { preventDefault: () => void }) => void,
    ): Promise<() => void> {
      // Browser tabs close via navigation; no Tauri close-request event.
      return noopUnlisten;
    },
    async setFocus() {},
    async show() {},
    async hide() {},
    async center() {},
    async setTitle() {},
    async startDragging() {},
  };
}

export function getCurrentWindow() {
  if (isTauriRuntime()) {
    return nativeGetCurrentWindow();
  }
  return remoteWindow();
}
