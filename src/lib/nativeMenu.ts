import { invoke } from "@tauri-apps/api/core";
import {
  currentLanguage,
  initLanguage,
  subscribeLanguage,
  type ResolvedLanguage,
} from "./i18n";

/**
 * The macOS menu bar lives outside the webview, so it cannot read the language
 * preference itself. The resolved tag is pushed across instead, on boot and on
 * every change. Windows and Linux draw their own menu bar in the webview, where
 * the command is a no-op.
 */
export function syncNativeMenuLanguage(language: ResolvedLanguage) {
  try {
    void invoke("set_menu_language", { language }).catch(() => {});
  } catch {
    // Not running under Tauri (unit tests, plain browser).
  }
}

/**
 * Applies the persisted language and keeps the native menu in step. Call once
 * during boot, before React renders.
 */
export function initNativeMenuLanguage() {
  initLanguage();
  syncNativeMenuLanguage(currentLanguage());
  return subscribeLanguage(() => {
    syncNativeMenuLanguage(currentLanguage());
  });
}
