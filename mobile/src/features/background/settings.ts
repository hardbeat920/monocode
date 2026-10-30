export const EFFECTS = [
  "none",
  "dither",
  "ascii",
  "halftone",
  "scanlines",
  "gradient-blur",
] as const;
export type BackgroundEffect = (typeof EFFECTS)[number];
export const EFFECT_LABELS: Record<BackgroundEffect, string> = {
  none: "None",
  dither: "Dither",
  ascii: "ASCII",
  halftone: "Halftone",
  scanlines: "Scanlines",
  "gradient-blur": "Haze",
};
export type BackgroundSettings = {
  image: string | null;
  opacity: number;
  sessionOpacity: number;
  scope: "empty" | "all";
  effect: BackgroundEffect;
};
export const OPACITY_RANGE = { min: 0.05, max: 0.65 } as const;
export function clampOpacity(value: unknown, fallback = 0.24) {
  return typeof value === "number" && Number.isFinite(value)
    ? Math.max(OPACITY_RANGE.min, Math.min(OPACITY_RANGE.max, value))
    : fallback;
}
export function normalizeBackground(value: unknown): BackgroundSettings {
  const raw =
    value && typeof value === "object"
      ? (value as Record<string, unknown>)
      : {};
  // Existing mobile photos were dithered, empty-only, at the saved opacity.
  const image =
    typeof raw.image === "string" &&
    raw.image.length > 0 &&
    !/[\\/]/.test(raw.image)
      ? raw.image
      : null;
  const legacy = !!image && !raw.effect;
  return {
    image,
    opacity: clampOpacity(raw.opacity, legacy ? 0.45 : 0.24),
    sessionOpacity: clampOpacity(raw.sessionOpacity),
    scope:
      raw.scope === "empty" || raw.scope === "all"
        ? raw.scope
        : legacy
          ? "empty"
          : "all",
    effect: EFFECTS.includes(raw.effect as BackgroundEffect)
      ? (raw.effect as BackgroundEffect)
      : legacy
        ? "dither"
        : "none",
  };
}
export function backgroundOpacity(
  settings: BackgroundSettings,
  empty: boolean,
) {
  return empty
    ? settings.opacity
    : settings.scope === "all"
      ? settings.sessionOpacity
      : 0;
}
