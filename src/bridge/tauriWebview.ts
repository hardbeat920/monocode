import { isTauriRuntime } from "./isTauri";
import { getCurrentWebview as nativeGetCurrentWebview } from "tauri-native-webview";

function remoteWebview() {
  return {
    async onDragDropEvent() {
      return () => {};
    },
  };
}

export function getCurrentWebview() {
  if (isTauriRuntime()) {
    return nativeGetCurrentWebview();
  }
  return remoteWebview();
}
