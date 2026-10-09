import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { hexToHsl, isHexColor } from "../../../shared/lib/colorUtils";
import {
  applyAccentColor,
  applyThemeDarkLightness,
  applyThemePreference,
  applyThemeTint,
  type ColorScheme,
  saveAccentColor,
  saveThemeDarkLightness,
  saveThemeHue,
  saveThemePreference,
  saveThemeSaturation,
  THEME_DARK_LIGHTNESS_MAX,
  THEME_DARK_LIGHTNESS_MIN,
} from "./appearance";

/**
 * A `theme.json` beside MonoCode's config lets a desktop theme switcher drive
 * the same Appearance settings a person sets by hand:
 *
 *   { "appearance": "dark", "background": "#111c18", "accent": "#509475" }
 *
 * Every field is optional. The file is applied when it changes, and once at
 * launch if it changed while MonoCode was closed, so manual tweaks made after
 * a theme switch stick until the next switch.
 */
export const EXTERNAL_THEME_CHANGED_EVENT = "monocode:external-theme-changed";

const APPLIED_KEY = "monocode.externalThemeApplied";

export type ExternalTheme = {
  appearance?: ColorScheme;
  background?: string;
  accent?: string;
};

export function parseExternalTheme(raw: string): ExternalTheme | null {
  let value: unknown;
  try {
    value = JSON.parse(raw);
  } catch {
    return null;
  }
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  const record = value as Record<string, unknown>;
  const theme: ExternalTheme = {};
  if (record.appearance === "dark" || record.appearance === "light") {
    theme.appearance = record.appearance;
  }
  if (typeof record.background === "string" && isHexColor(record.background)) {
    theme.background = record.background.toLowerCase();
  }
  if (typeof record.accent === "string" && isHexColor(record.accent)) {
    theme.accent = record.accent.toLowerCase();
  }
  return theme;
}

export function applyExternalTheme(theme: ExternalTheme) {
  if (theme.background) {
    const { h, s, l } = hexToHsl(theme.background);
    saveThemeHue(h);
    saveThemeSaturation(s);
    applyThemeTint(h, s);
    // Light mode keeps its fixed page lightness; only dark mode is tunable.
    if (theme.appearance !== "light") {
      const lightness = Math.min(
        THEME_DARK_LIGHTNESS_MAX,
        Math.max(THEME_DARK_LIGHTNESS_MIN, l),
      );
      saveThemeDarkLightness(lightness);
      applyThemeDarkLightness(lightness);
    }
  }
  if (theme.accent) {
    saveAccentColor(theme.accent);
    applyAccentColor(theme.accent);
  }
  if (theme.appearance) {
    saveThemePreference(theme.appearance);
    applyThemePreference(theme.appearance);
  }
}

function readApplied(): string | null {
  try {
    return localStorage.getItem(APPLIED_KEY);
  } catch {
    return null;
  }
}

function writeApplied(raw: string) {
  try {
    localStorage.setItem(APPLIED_KEY, raw);
  } catch {
    // private mode / quota
  }
}

/** Applies `raw` unless it is the file this install last applied. */
export function applyExternalThemeSource(
  raw: string | null,
  { onlyIfNew }: { onlyIfNew: boolean },
) {
  if (raw == null) return;
  if (onlyIfNew && readApplied() === raw) return;
  const theme = parseExternalTheme(raw);
  if (!theme) return;
  applyExternalTheme(theme);
  writeApplied(raw);
}

export function initExternalTheme() {
  void invoke<string | null>("read_external_theme")
    .then((raw) => applyExternalThemeSource(raw, { onlyIfNew: true }))
    .catch(() => {});
  // Every window hears the change, so each applies it even when another
  // window already recorded it as applied.
  void listen<string | null>(EXTERNAL_THEME_CHANGED_EVENT, (event) => {
    applyExternalThemeSource(event.payload, { onlyIfNew: false });
  }).catch(() => {});
}
