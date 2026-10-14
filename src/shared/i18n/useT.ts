import { useMemo, useSyncExternalStore } from "react";
import {
  getLocale,
  subscribeLocale,
  translate,
  type Locale,
  type MessageParams,
  type TranslateFn,
} from "./locale";
import type { MessageKey } from "./catalog";

/** The current locale. Components re-render when it changes. */
export function useLocale(): Locale {
  return useSyncExternalStore(subscribeLocale, getLocale, getLocale);
}

/**
 * Returns `t` bound to the current locale. Components that render text should
 * use this instead of the module-level `t`, which does not trigger re-renders.
 */
export function useT(): TranslateFn {
  const locale = useLocale();
  return useMemo(
    () =>
      ((key: MessageKey, params?: MessageParams) =>
        translate(locale, key, params)) as TranslateFn,
    [locale],
  );
}
