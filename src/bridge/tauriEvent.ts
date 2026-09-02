import { isTauriRuntime } from "./isTauri";
import { remoteListen } from "./remoteClient";
import { listen as nativeListen, type UnlistenFn } from "tauri-native-event";

export type { UnlistenFn };

export function listen<T>(
  event: string,
  handler: (event: { payload: T }) => void,
): Promise<UnlistenFn> {
  if (isTauriRuntime()) {
    return nativeListen<T>(event, handler);
  }
  return remoteListen<T>(event, handler);
}
