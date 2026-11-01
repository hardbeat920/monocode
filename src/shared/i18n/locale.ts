import { zhCN } from "./zh-CN";

export type Locale = "en" | "zh-CN";
export type LanguagePreference = "system" | Locale;
export type TranslationValues = Readonly<Record<string, string | number>>;

export const LANGUAGE_KEY = "xcode.language";
export const LANGUAGE_DEFAULT: LanguagePreference = "zh-CN";
export const LANGUAGE_CHANGE_EVENT = "xcode:languagechange";

let unsaved: LanguagePreference | null = null;

export function normalizeLanguage(value: unknown): LanguagePreference {
  return value === "en" || value === "zh-CN" ? value : LANGUAGE_DEFAULT;
}

export function loadLanguage(): LanguagePreference {
  if (unsaved !== null) return unsaved;
  try {
    return normalizeLanguage(localStorage.getItem(LANGUAGE_KEY));
  } catch {
    return LANGUAGE_DEFAULT;
  }
}

export function resolveLocale(
  language: LanguagePreference,
  systemLanguages: readonly string[],
): Locale {
  if (language !== "system") return language;
  return /^zh(?:-|$)/i.test(systemLanguages[0] ?? "") ? "zh-CN" : "en";
}

export function loadLocale(): Locale {
  const languages =
    typeof navigator === "undefined"
      ? []
      : navigator.languages?.length
        ? navigator.languages
        : [navigator.language];
  return resolveLocale(loadLanguage(), languages);
}

function syncDocumentLanguage() {
  if (typeof document !== "undefined") {
    document.documentElement.lang = loadLocale();
  }
}

export function saveLanguage(value: LanguagePreference) {
  const next = normalizeLanguage(value);
  try {
    localStorage.setItem(LANGUAGE_KEY, next);
    unsaved = null;
  } catch {
    unsaved = next;
  }
  syncDocumentLanguage();
  if (typeof window !== "undefined") {
    window.dispatchEvent(new Event(LANGUAGE_CHANGE_EVENT));
  }
}

export function subscribeLanguage(onChange: () => void) {
  if (typeof window === "undefined") return () => {};
  const onStorage = (event: StorageEvent) => {
    if (event.key !== LANGUAGE_KEY && event.key !== null) return;
    if (event.storageArea && event.storageArea !== window.localStorage) return;
    unsaved = null;
    syncDocumentLanguage();
    onChange();
  };
  const onSystemLanguage = () => {
    if (loadLanguage() !== "system") return;
    syncDocumentLanguage();
    onChange();
  };
  window.addEventListener(LANGUAGE_CHANGE_EVENT, onChange);
  window.addEventListener("storage", onStorage);
  window.addEventListener("languagechange", onSystemLanguage);
  return () => {
    window.removeEventListener(LANGUAGE_CHANGE_EVENT, onChange);
    window.removeEventListener("storage", onStorage);
    window.removeEventListener("languagechange", onSystemLanguage);
  };
}

/** Call once per webview before rendering. */
export function initLocale() {
  syncDocumentLanguage();
  return subscribeLanguage(syncDocumentLanguage);
}

/** English source text is the key; missing translations stay English. */
export function translate(
  message: string,
  values?: TranslationValues,
  locale: Locale = loadLocale(),
): string {
  const translated =
    locale === "zh-CN" && Object.prototype.hasOwnProperty.call(zhCN, message)
      ? zhCN[message]
      : message;
  if (!values) return translated;
  return translated.replace(/\{(\w+)\}/g, (placeholder, key: string) =>
    Object.prototype.hasOwnProperty.call(values, key)
      ? String(values[key])
      : placeholder,
  );
}
