import { isTauriRuntime } from "./isTauri";
import { remoteInvoke } from "./remoteClient";
import * as native from "tauri-native-core";

export const Channel = native.Channel;
export const Resource = native.Resource;
export const SERIALIZE_TO_IPC_FN = native.SERIALIZE_TO_IPC_FN;
export const transformCallback = native.transformCallback;

export async function invoke<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  if (isTauriRuntime()) {
    return native.invoke<T>(command, args);
  }
  return remoteInvoke<T>(command, args ?? {});
}

export function convertFileSrc(filePath: string, protocol: string = "asset") {
  if (isTauriRuntime()) {
    return native.convertFileSrc(filePath, protocol);
  }
  return filePath;
}
