import {
  ask as tauriAsk,
  message as tauriMessage,
  open as tauriOpen,
  type OpenDialogOptions,
} from "@tauri-apps/plugin-dialog";

/**
 * Companion-safe dialogs.
 *
 * Native-first: on desktop and in the Tauri iOS shell these are the exact
 * Tauri sheets/pickers as before (plugin calls run against the *local*
 * shell, so a confirm on the iPad never pops a sheet on the Mac). Anywhere
 * the plugin is missing — pure-web preview builds, or a future PWA — they
 * degrade to blocking browser primitives (confirm/alert) or a null pick,
 * which every caller already handles as "dismissed". New code should import
 * from here, never from `@tauri-apps/plugin-dialog` directly.
 */

export type DialogOptions = {
  title?: string;
  kind?: "info" | "warning" | "error";
  okLabel?: string;
};

export async function askDialog(
  message: string,
  options?: DialogOptions,
): Promise<boolean> {
  try {
    return await tauriAsk(message, options);
  } catch {
    return window.confirm(message);
  }
}

export async function messageDialog(
  message: string,
  options?: DialogOptions,
): Promise<void> {
  try {
    await tauriMessage(message, options);
  } catch {
    window.alert(message);
  }
}

export type OpenResult = string | string[] | null;

export async function openDialog(
  options: OpenDialogOptions,
): Promise<OpenResult> {
  try {
    const selected = await tauriOpen(options);
    if (selected == null) return null;
    return Array.isArray(selected) ? selected.map(String) : String(selected);
  } catch {
    // No native picker (pure web): callers treat null as "no selection".
    return null;
  }
}
