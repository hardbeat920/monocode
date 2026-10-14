import {
  en,
  tr,
  type MessageKey,
  type MessageValue,
  type PluralKey,
  type TextKey,
} from "./catalog";

export type Locale = "en" | "tr";

/** What the user picked. `system` follows the OS language at startup. */
export type LocalePreference = Locale | "system";

export const LOCALES: readonly Locale[] = ["en", "tr"];

export const LOCALE_PREFERENCE_KEY = "monocode.language";
export const LOCALE_PREFERENCE_DEFAULT: LocalePreference = "system";

/** Names are written in their own language so they stay readable from any locale. */
export const LOCALE_NAMES: Record<Locale, string> = {
  en: "English",
  tr: "Türkçe",
};

const LOCALE_TAGS: Record<Locale, string> = {
  en: "en-US",
  tr: "tr-TR",
};

const CATALOGS: Record<Locale, Record<string, MessageValue>> = { en, tr };

export type MessageParams = Record<string, string | number>;

export interface TranslateFn {
  (key: TextKey, params?: MessageParams): string;
  (key: PluralKey, params: MessageParams & { count: number }): string;
}

export function isLocalePreference(value: unknown): value is LocalePreference {
  return value === "system" || value === "en" || value === "tr";
}

/**
 * Picks the first language in the OS preference list that MonoCode ships.
 * Anything else falls back to English, the source catalog.
 */
export function resolveSystemLocale(
  languages: readonly string[] = typeof navigator === "undefined"
    ? []
    : navigator.languages,
): Locale {
  for (const language of languages) {
    const primary = language.split(/[-_]/)[0]?.toLowerCase();
    if (primary === "tr") return "tr";
    if (primary === "en") return "en";
  }
  return "en";
}

export function resolveLocale(preference: LocalePreference): Locale {
  return preference === "system" ? resolveSystemLocale() : preference;
}

function readStoredPreference(): LocalePreference {
  try {
    const raw = localStorage.getItem(LOCALE_PREFERENCE_KEY);
    return isLocalePreference(raw) ? raw : LOCALE_PREFERENCE_DEFAULT;
  } catch {
    return LOCALE_PREFERENCE_DEFAULT;
  }
}

let preference: LocalePreference = readStoredPreference();
let current: Locale = resolveLocale(preference);
const listeners = new Set<() => void>();

export function getLocale(): Locale {
  return current;
}

export function getLocalePreference(): LocalePreference {
  return preference;
}

/** BCP 47 tag for `Intl` and `toLocale*String` calls. */
export function getLocaleTag(): string {
  return LOCALE_TAGS[current];
}

export function subscribeLocale(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

function refreshLocale() {
  const next = resolveLocale(preference);
  if (next === current) return;
  current = next;
  if (typeof document !== "undefined") {
    document.documentElement.lang = next;
  }
  for (const listener of listeners) listener();
}

/**
 * Stores the preference and switches every consumer of this module. Other
 * MonoCode windows pick it up through the `storage` event in `initLocale`.
 */
export function setLocalePreference(next: LocalePreference) {
  preference = next;
  try {
    localStorage.setItem(LOCALE_PREFERENCE_KEY, next);
  } catch {
    // private mode / quota: the choice still applies for this window
  }
  refreshLocale();
}

/**
 * Sets `<html lang>` and keeps this window in sync with preference changes
 * made in other windows. Call once per window entry point before rendering.
 */
export function initLocale() {
  if (typeof document !== "undefined") {
    document.documentElement.lang = current;
  }
  if (typeof window !== "undefined") {
    window.addEventListener("storage", (event) => {
      if (event.key !== LOCALE_PREFERENCE_KEY) return;
      preference = readStoredPreference();
      refreshLocale();
    });
  }
}

/** Picks the singular or plural form for `params.count`, then fills `{name}` slots. */
export function formatMessage(
  entry: MessageValue,
  locale: Locale,
  params: MessageParams = {},
): string {
  let message: string;
  if (typeof entry === "string") {
    message = entry;
  } else {
    const count = Number(params.count ?? 0);
    const rule = new Intl.PluralRules(LOCALE_TAGS[locale]).select(count);
    message = rule === "one" ? entry.one : entry.other;
  }
  return message.replace(/\{(\w+)\}/g, (match, name: string) =>
    name in params ? String(params[name]) : match,
  );
}

export function translate(
  locale: Locale,
  key: MessageKey,
  params: MessageParams = {},
): string {
  return formatMessage(CATALOGS[locale][key] ?? en[key], locale, params);
}

/**
 * Translates with the current locale. Outside React, call it directly. In
 * components, use `useT()` so the text updates when the language changes.
 */
export const t: TranslateFn = (key: MessageKey, params?: MessageParams) =>
  translate(current, key, params);
