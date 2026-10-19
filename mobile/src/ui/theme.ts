import Storage from "expo-sqlite/kv-store";
import { useSyncExternalStore } from "react";
import { StyleSheet, useColorScheme } from "react-native";

export type Scheme = "light" | "dark";

// The desktop dark theme in src/styles/index.css (neutral hue, blue accent), on a
// darker base for mobile: hsl(0 0% 6%) rather than desktop's 9%. The drawer shares
// the base, and the scene lightens as it slides aside to set itself apart; bottom
// sheets sit a step above, like raised iOS sheets.
const dark = {
  background: "#0f0f0f",
  drawer: "#0f0f0f",
  sheet: "#141414",
  surface: "#1c1c1c",
  surfaceRaised: "#262626",
  stroke: "rgba(235, 235, 235, 0.08)",
  selection: "rgba(235, 235, 235, 0.10)",
  content: "#ebebeb",
  muted: "#8c8c8c",
  faint: "#5c5c5c",
  accent: "#4c9ef5",
  additions: "#6ee7b7",
  deletions: "#f87171",
  /** Amber, for warnings and full access. */
  caution: "#fbbf24",
  /** A file named in a transcript, apart from prose, code and links. */
  fileRef: "#c4b5fd",
  inverse: "#0c0c0c",
  inverseSurface: "#f2f2f2",
  /** Round buttons and pills over glass, as the composer's. */
  fill: "rgba(255, 255, 255, 0.08)",
  /** A segmented switch's thumb, inside the glass. */
  thumb: "rgba(255, 255, 255, 0.14)",
  /** Laid over the Monocode mark, which is drawn white. */
  mark: "#ffffff",
  /** A transcript's fold rails and connectors. */
  rail: "#2e2e2e",
  /** The user's message bubble. */
  bubble: "#252525",
  /** The file list's card, and the lines between its rows. */
  listCard: "#1a1a1a",
  listDivider: "#2b2b2b",
};

export type Palette = typeof dark;

// The desktop's `html.theme-light`: an hsl(0 0% 97%) base under 18% ink, with
// gentler fills and darker diff and accent hues to hold their contrast. Sheets
// and raised surfaces go white, as iOS lifts them in light mode.
const light: Palette = {
  background: "#f7f7f7",
  drawer: "#f7f7f7",
  sheet: "#ffffff",
  surface: "#ededed",
  surfaceRaised: "#e3e3e3",
  stroke: "rgba(46, 46, 46, 0.10)",
  selection: "rgba(46, 46, 46, 0.06)",
  content: "#2e2e2e",
  muted: "#737373",
  faint: "#a6a6a6",
  accent: "#1f7ae0",
  additions: "#047857",
  deletions: "#be123c",
  caution: "#b45309",
  fileRef: "#6d28d9",
  inverse: "#fafafa",
  inverseSurface: "#262626",
  // Light glass washes faint ink out, so these are heavier than dark's.
  fill: "rgba(46, 46, 46, 0.09)",
  thumb: "rgba(46, 46, 46, 0.10)",
  mark: "#2e2e2e",
  rail: "#dadada",
  bubble: "#e9e9e9",
  listCard: "#ffffff",
  listDivider: "#e6e6e6",
};

export const palettes: Record<Scheme, Palette> = { dark, light };

/** `hsl()` as `#rrggbb`, so tinted colours still take `withAlpha`. */
function hsl(hue: number, saturation: number, lightness: number) {
  const s = saturation / 100;
  const l = lightness / 100;
  const k = (n: number) => (n + hue / 30) % 12;
  const a = s * Math.min(l, 1 - l);
  const channel = (n: number) =>
    Math.round(255 * (l - a * Math.max(-1, Math.min(k(n) - 3, 9 - k(n), 1))))
      .toString(16)
      .padStart(2, "0");
  return `#${channel(0)}${channel(8)}${channel(4)}`;
}

/** The neutral palette's surfaces, washed in a hue: dark's barely, so text
 *  keeps its contrast, and light's fainter still. */
function tinted(hue: number, accent: Record<Scheme, string>): Record<Scheme, Palette> {
  return {
    dark: {
      ...dark,
      background: hsl(hue, 22, 7),
      drawer: hsl(hue, 22, 7),
      sheet: hsl(hue, 20, 9),
      surface: hsl(hue, 16, 12.5),
      surfaceRaised: hsl(hue, 14, 16.5),
      rail: hsl(hue, 12, 19),
      bubble: hsl(hue, 16, 16),
      listCard: hsl(hue, 18, 11),
      listDivider: hsl(hue, 14, 18),
      accent: accent.dark,
    },
    light: {
      ...light,
      background: hsl(hue, 30, 96.5),
      drawer: hsl(hue, 30, 96.5),
      surface: hsl(hue, 22, 92.5),
      surfaceRaised: hsl(hue, 18, 88.5),
      rail: hsl(hue, 16, 85),
      bubble: hsl(hue, 22, 91),
      listDivider: hsl(hue, 16, 89.5),
      accent: accent.light,
    },
  };
}

/**
 * The themes the app can be drawn in, each in both schemes: Classic, the
 * desktop's neutral greys, and the rest washing its surfaces in a hue with an
 * accent to match. `orb` gives each scheme's swatch in the picker, its
 * highlight and its body.
 */
export const THEMES = {
  mono: {
    label: "Classic",
    palettes,
    orb: { light: ["#ffffff", "#b9c6f5"], dark: ["#3a3d4f", "#0c0c0c"] },
  },
  ocean: {
    label: "Cobalt",
    palettes: tinted(212, { dark: "#5aa9f7", light: "#1f6fc4" }),
    orb: { light: ["#ffffff", "#5c9be0"], dark: ["#6fb4ee", "#14283a"] },
  },
  grove: {
    label: "Jade",
    palettes: tinted(150, { dark: "#4cc38a", light: "#13804f" }),
    orb: { light: ["#f6f1e2", "#4f9d6e"], dark: ["#7ad9a0", "#1a2b20"] },
  },
  ember: {
    label: "Copper",
    palettes: tinted(22, { dark: "#f0884a", light: "#c2531a" }),
    orb: { light: ["#fbefe8", "#cf7a45"], dark: ["#e79a62", "#2e1a12"] },
  },
  iris: {
    label: "Violet",
    palettes: tinted(262, { dark: "#9d87f5", light: "#6a4fd6" }),
    orb: { light: ["#f6effc", "#9a7ee0"], dark: ["#9b7cf0", "#1d1730"] },
  },
  rose: {
    label: "Blush",
    palettes: tinted(338, { dark: "#ec6fa3", light: "#c42a6c" }),
    orb: { light: ["#fdf0f5", "#de6b9a"], dark: ["#c94b7e", "#2b1220"] },
  },
} satisfies Record<
  string,
  { label: string; palettes: Record<Scheme, Palette>; orb: Record<Scheme, [string, string]> }
>;

export type ThemeName = keyof typeof THEMES;

const THEME_KEY = "theme";

function loadTheme(): ThemeName {
  try {
    const stored = Storage.getItemSync(THEME_KEY);
    return stored && stored in THEMES ? (stored as ThemeName) : "mono";
  } catch {
    return "mono";
  }
}

// The theme in use, held here so every screen redraws in it together.
let theme = loadTheme();
const themeListeners = new Set<() => void>();
const subscribeTheme = (listener: () => void) => {
  themeListeners.add(listener);
  return () => {
    themeListeners.delete(listener);
  };
};

/** Draws the app in `next`, and remembers it on this phone. */
export function setTheme(next: ThemeName) {
  if (next === theme) return;
  theme = next;
  void Storage.setItem(THEME_KEY, next);
  themeListeners.forEach((listener) => listener());
}

/** The theme the app is drawn in. */
export function useTheme(): ThemeName {
  return useSyncExternalStore(subscribeTheme, () => theme);
}

/** A theme's palette in a scheme. */
export function paletteFor(scheme: Scheme, name: ThemeName = theme): Palette {
  return THEMES[name].palettes[scheme];
}

export const radii = {
  pill: 999,
  card: 22,
  row: 12,
} as const;

/** The scheme the app is drawn in: the phone's, unless overridden in settings. */
export function useScheme(): Scheme {
  return useColorScheme() === "light" ? "light" : "dark";
}

/** The palette of the scheme and theme the app is drawn in. */
export function useColors() {
  return paletteFor(useScheme(), useTheme());
}

/**
 * A style sheet for each scheme and theme, built from its palette, as a hook
 * returning the one in use. Each is built the first time it is drawn and
 * kept, so switching back costs nothing.
 */
export function themedStyles<T extends StyleSheet.NamedStyles<T>>(
  build: (colors: Palette) => T,
) {
  const sheets = new Map<string, T>();
  return function useStyles() {
    const scheme = useScheme();
    const name = useTheme();
    const key = `${name}.${scheme}`;
    let sheet = sheets.get(key);
    if (!sheet) {
      sheet = StyleSheet.create(build(paletteFor(scheme, name)));
      sheets.set(key, sheet);
    }
    return sheet;
  };
}

/** `#rrggbb` to `rgba()` at the given alpha. */
export function withAlpha(hex: string, alpha: number) {
  const n = parseInt(hex.slice(1), 16);
  return `rgba(${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255}, ${alpha})`;
}
