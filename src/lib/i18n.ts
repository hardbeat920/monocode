/**
 * Lightweight i18n module for MonoCode.
 *
 * English strings are used as keys. The default locale is "en" (identity
 * mapping). To add a new language, create a locale file that maps every
 * English key to the translated string, register it here, and call
 * setLocale() at app startup.
 */

import { en } from "./locales/en";
import { zhCN } from "./locales/zh-CN";

export type Locale = Record<string, string>;

const locales: Record<string, Locale> = {
  en,
  "zh-CN": zhCN,
};

let currentLocale = "en";

export function setLocale(locale: string) {
  currentLocale = locale;
}

export function getLocale(): string {
  return currentLocale;
}

/**
 * Translate an English key to the current locale.
 * Supports `{0}`, `{1}`, … positional placeholders.
 *
 * @example t("No results")
 * @example t("Create and checkout {0}", [branchName])
 */
export function t(key: string, params?: (string | number)[]): string {
  const dict = locales[currentLocale];
  const template = dict?.[key] ?? key;
  if (!params || params.length === 0) return template;
  return template.replace(/\{(\d+)\}/g, (_, i: string) => {
    const idx = Number(i);
    return idx < params.length ? String(params[idx]) : `{${idx}}`;
  });
}

/**
 * Translate a template string with named placeholders.
 * Keys in `params` replace `{key}` in the translated template.
 *
 * @example tNamed("Update to {version}", { version: "1.2.3" })
 */
export function tNamed(key: string, params: Record<string, string | number>): string {
  const dict = locales[currentLocale];
  const template = dict?.[key] ?? key;
  return template.replace(/\{(\w+)\}/g, (_, name: string) =>
    name in params ? String(params[name]) : `{${name}}`,
  );
}

/**
 * Generate a combined plural key from separate singular and plural forms.
 *
 * @example pluralKey("{0} file", "{0} files") // "{0} file|{0} files"
 */
export function pluralKey(singular: string, plural: string): string {
  return `${singular}|${plural}`;
}

/**
 * Translate a plural-aware key for the given count.
 *
 * The key is looked up in the current locale; the translated value should
 * contain singular and plural forms separated by `|` (e.g. `"1 个文件|{0} 个文件"`).
 * When the key is not found in the locale, the raw key itself is used as the
 * template.
 *
 * - `count === 1` selects the singular form (left of `|`)
 * - otherwise selects the plural form (right of `|`)
 *
 * `{0}` in the chosen form is replaced with the count value.
 *
 * @example tPlural("{0} file|{0} files", 1) // "1 个文件"  (zh-CN locale)
 * @example tPlural("{0} file|{0} files", 3) // "3 个文件"
 */
export function tPlural(key: string, count: number): string {
  const dict = locales[currentLocale];
  const template = dict?.[key] ?? key;
  const parts = template.split("|");
  const form = count === 1 ? parts[0] : (parts[1] ?? parts[0]);
  return form.replace(/\{0\}/g, String(count));
}
