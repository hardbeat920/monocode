import { useEffect, useState } from "react";
import {
  loadThemePreference,
  resolveColorScheme,
  SCHEME_CHANGE_EVENT,
  type ColorScheme,
} from "../../features/settings/model/appearance";

/** Subscribes to color scheme changes triggered by applyThemePreference(). */
export function useColorScheme(): ColorScheme {
  const [scheme, setScheme] = useState<ColorScheme>(() =>
    resolveColorScheme(loadThemePreference()),
  );
  useEffect(() => {
    const onChange = (event: Event) => {
      const detail = (event as CustomEvent<ColorScheme>).detail;
      setScheme(detail === "light" ? "light" : "dark");
    };
    window.addEventListener(SCHEME_CHANGE_EVENT, onChange);
    return () => window.removeEventListener(SCHEME_CHANGE_EVENT, onChange);
  }, []);
  return scheme;
}
