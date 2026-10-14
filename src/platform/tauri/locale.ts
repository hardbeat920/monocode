import { invoke } from "@tauri-apps/api/core";
import type { Locale } from "../../shared/i18n/locale";

/**
 * Tells the Rust side which language to use for native menus, the tray, and
 * notifications. Failures are ignored: the app stays usable in English.
 */
export function syncNativeLocale(locale: Locale): Promise<void> {
  return invoke<void>("set_app_locale", { locale }).catch(() => undefined);
}
