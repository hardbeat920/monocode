import { useSyncExternalStore } from "react";
import { en, type TranslationDict } from "../locales/en";
import { zhCN } from "../locales/zh-CN";

export type LanguagePreference = "system" | "en" | "zh-CN";
export type ResolvedLanguage = "en" | "zh-CN";

const LANGUAGE_KEY = "monocode.languagePreference";
export const LANGUAGE_CHANGE_EVENT = "monocode:language-change";

export const LANGUAGE_OPTIONS: { id: LanguagePreference; label: string }[] = [
  { id: "system", label: "System Default / 跟随系统" },
  { id: "zh-CN", label: "简体中文 (Chinese Simplified)" },
  { id: "en", label: "English" },
];

export function isLanguagePreference(
  value: unknown,
): value is LanguagePreference {
  return value === "system" || value === "en" || value === "zh-CN";
}

export function loadLanguagePreference(): LanguagePreference {
  try {
    const raw = localStorage.getItem(LANGUAGE_KEY);
    return isLanguagePreference(raw) ? raw : "system";
  } catch {
    return "system";
  }
}

export function saveLanguagePreference(value: LanguagePreference) {
  try {
    localStorage.setItem(LANGUAGE_KEY, value);
    if (typeof window !== "undefined") {
      window.dispatchEvent(
        new CustomEvent(LANGUAGE_CHANGE_EVENT, { detail: value }),
      );
    }
  } catch {
    // quota / private browsing
  }
}

export function resolveLanguage(pref?: LanguagePreference): ResolvedLanguage {
  const choice = pref ?? loadLanguagePreference();
  if (choice === "en") return "en";
  if (choice === "zh-CN") return "zh-CN";

  // System detection
  if (typeof navigator !== "undefined") {
    if (navigator.language && navigator.language.toLowerCase().startsWith("zh")) {
      return "zh-CN";
    }
    if (Array.isArray(navigator.languages)) {
      for (const lang of navigator.languages) {
        if (lang.toLowerCase().startsWith("zh")) return "zh-CN";
      }
    }
  }
  return "zh-CN";
}

const LOCALES: Record<ResolvedLanguage, TranslationDict> = {
  en,
  "zh-CN": zhCN,
};

function getNestedValue(obj: unknown, path: string): string | undefined {
  const parts = path.split(".");
  let current: any = obj;
  for (const part of parts) {
    if (current == null || typeof current !== "object") return undefined;
    current = current[part];
  }
  return typeof current === "string" ? current : undefined;
}

/**
 * Look up a translated string by dot-notated key.
 * If not found in the resolved language, falls back to English, then to provided fallback, then to key itself.
 */
export function t(
  key: string,
  fallback?: string,
  vars?: Record<string, string | number>,
): string {
  const resolved = resolveLanguage();
  const dict = LOCALES[resolved];
  let text = getNestedValue(dict, key);

  if (text == null && resolved !== "en") {
    text = getNestedValue(LOCALES.en, key);
  }

  if (text == null) {
    text = fallback ?? key;
  }

  if (vars) {
    for (const [vKey, vVal] of Object.entries(vars)) {
      text = text.replace(new RegExp(`\\{${vKey}\\}`, "g"), String(vVal));
    }
  }

  return text;
}

let cachedSnapshot: {
  pref: LanguagePreference;
  resolved: ResolvedLanguage;
} | null = null;

function getLanguageSnapshot() {
  const pref = loadLanguagePreference();
  const resolved = resolveLanguage(pref);
  if (
    !cachedSnapshot ||
    cachedSnapshot.pref !== pref ||
    cachedSnapshot.resolved !== resolved
  ) {
    cachedSnapshot = { pref, resolved };
  }
  return cachedSnapshot;
}

function subscribeLanguage(callback: () => void): () => void {
  if (typeof window === "undefined") return () => {};
  const handler = () => {
    cachedSnapshot = null;
    callback();
  };
  window.addEventListener(LANGUAGE_CHANGE_EVENT, handler);
  window.addEventListener("storage", handler);
  return () => {
    window.removeEventListener(LANGUAGE_CHANGE_EVENT, handler);
    window.removeEventListener("storage", handler);
  };
}

export function useTranslation() {
  const snapshot = useSyncExternalStore(
    subscribeLanguage,
    getLanguageSnapshot,
    getLanguageSnapshot,
  );

  const translate = (
    key: string,
    fallback?: string,
    vars?: Record<string, string | number>,
  ) => {
    const dict = LOCALES[snapshot.resolved];
    let text = getNestedValue(dict, key);
    if (text == null && snapshot.resolved !== "en") {
      text = getNestedValue(LOCALES.en, key);
    }
    if (text == null) {
      text = fallback ?? key;
    }
    if (vars) {
      for (const [vKey, vVal] of Object.entries(vars)) {
        text = text.replace(new RegExp(`\\{${vKey}\\}`, "g"), String(vVal));
      }
    }
    return text;
  };

  return {
    t: translate,
    languagePreference: snapshot.pref,
    resolvedLanguage: snapshot.resolved,
    setLanguage: saveLanguagePreference,
  };
}
