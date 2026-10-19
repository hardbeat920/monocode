import { useSyncExternalStore } from "react";
import { zhCN } from "./zh-CN";

/** English is the source language and the default until the user chooses another. */
export const APP_LOCALES = ["en", "zh-CN"] as const;

export type AppLocale = (typeof APP_LOCALES)[number];

export const LOCALE_OPTIONS: { id: AppLocale; nativeLabel: string }[] = [
  { id: "en", nativeLabel: "English" },
  { id: "zh-CN", nativeLabel: "简体中文" },
];

const STORAGE_KEY = "monocode.locale";
const CHANGE_EVENT = "monocode:localechange";

const CATALOGS: Partial<Record<AppLocale, Record<string, string>>> = {
  "zh-CN": zhCN,
};

/** A value that could not be written, kept for this window only. */
let unsaved: AppLocale | null = null;

export function isAppLocale(value: unknown): value is AppLocale {
  return value === "en" || value === "zh-CN";
}

export function loadLocale(): AppLocale {
  if (unsaved) return unsaved;
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    return isAppLocale(raw) ? raw : "en";
  } catch {
    return "en";
  }
}

export function saveLocale(locale: AppLocale) {
  try {
    localStorage.setItem(STORAGE_KEY, locale);
    unsaved = localStorage.getItem(STORAGE_KEY) === locale ? null : locale;
  } catch {
    unsaved = locale;
  }
  applyDocumentLocale(locale);
  if (typeof window === "undefined") return;
  window.dispatchEvent(new CustomEvent<AppLocale>(CHANGE_EVENT, { detail: locale }));
}

export function applyDocumentLocale(locale: AppLocale = loadLocale()) {
  if (typeof document === "undefined") return;
  document.documentElement.lang = locale;
}

function subscribe(onStoreChange: () => void) {
  if (typeof window === "undefined") return () => {};
  const onStorage = (event: StorageEvent) => {
    if (event.key !== STORAGE_KEY && event.key !== null) return;
    unsaved = null;
    applyDocumentLocale();
    onStoreChange();
  };
  window.addEventListener(CHANGE_EVENT, onStoreChange);
  window.addEventListener("storage", onStorage);
  return () => {
    window.removeEventListener(CHANGE_EVENT, onStoreChange);
    window.removeEventListener("storage", onStorage);
  };
}

export function useLocale(): AppLocale {
  return useSyncExternalStore(subscribe, loadLocale, () => "en");
}

/**
 * Looks up an English source string. Missing entries stay in English, so a
 * new screen can ship before every language has a translation.
 */
export function translate(
  message: string,
  values?: Record<string, string | number>,
  locale: AppLocale = loadLocale(),
): string {
  const catalog = CATALOGS[locale];
  const template =
    catalog && Object.prototype.hasOwnProperty.call(catalog, message)
      ? catalog[message]
      : message;
  if (!values) return template;
  return template.replace(/\{(\w+)\}/g, (token, key: string) => {
    const value = values[key];
    return value == null ? token : String(value);
  });
}

export function useTranslate() {
  const locale = useLocale();
  return (message: string, values?: Record<string, string | number>) =>
    translate(message, values, locale);
}
