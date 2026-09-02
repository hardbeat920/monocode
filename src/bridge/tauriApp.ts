import { isTauriRuntime } from "./isTauri";
import { getVersion as nativeGetVersion } from "tauri-native-app";

export function getVersion(): Promise<string> {
  if (isTauriRuntime()) {
    return nativeGetVersion();
  }
  return Promise.resolve("remote");
}
