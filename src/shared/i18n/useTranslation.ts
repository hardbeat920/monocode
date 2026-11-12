import { useCallback, useSyncExternalStore } from "react";
import {
  LANGUAGE_DEFAULT,
  loadLanguage,
  loadLocale,
  subscribeLanguage,
  translate,
  type Locale,
  type TranslationValues,
} from "./locale";

export function useLanguage() {
  return useSyncExternalStore(
    subscribeLanguage,
    loadLanguage,
    () => LANGUAGE_DEFAULT,
  );
}

export function useTranslation() {
  const locale = useSyncExternalStore<Locale>(
    subscribeLanguage,
    loadLocale,
    () => "en",
  );
  const t = useCallback(
    (message: string, values?: TranslationValues) =>
      translate(message, values, locale),
    [locale],
  );
  return { t, locale };
}
