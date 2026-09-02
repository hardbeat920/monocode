import { isTauriRuntime } from "./isTauri";
import {
  ask as nativeAsk,
  message as nativeMessage,
  open as nativeOpen,
} from "tauri-native-dialog";

export function ask(
  message: string,
  options?: { title?: string; kind?: "info" | "warning" | "error" },
): Promise<boolean> {
  if (isTauriRuntime()) {
    return nativeAsk(message, options);
  }
  return Promise.resolve(
    window.confirm(
      options?.title ? `${options.title}\n\n${message}` : message,
    ),
  );
}

export async function message(
  text: string,
  options?: { title?: string; kind?: "info" | "warning" | "error" },
): Promise<void> {
  if (isTauriRuntime()) {
    await nativeMessage(text, options);
    return;
  }
  window.alert(options?.title ? `${options.title}\n\n${text}` : text);
}

export function open(options?: {
  directory?: boolean;
  multiple?: boolean;
  defaultPath?: string;
}): Promise<string | string[] | null> {
  if (isTauriRuntime()) {
    return nativeOpen(options);
  }
  return Promise.resolve(null);
}
