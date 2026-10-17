import { useSyncExternalStore } from "react";
import {
  currentLanguage,
  subscribeLanguage,
  t,
  type ResolvedLanguage,
} from "../lib/i18n";

/**
 * Subscribes the calling component to language changes and returns `t`.
 * Components that only need the resolved language for memo deps or branching
 * can use `useLanguage` instead.
 */
export function useT() {
  useSyncExternalStore(subscribeLanguage, currentLanguage, currentLanguage);
  return t;
}

/** The resolved language ("en" | "zh"), reactive to preference changes. */
export function useLanguage(): ResolvedLanguage {
  return useSyncExternalStore(subscribeLanguage, currentLanguage, currentLanguage);
}
