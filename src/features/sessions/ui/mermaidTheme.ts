import type { MermaidConfig } from "@streamdown/mermaid";

type Rgb = readonly [number, number, number];

export type MermaidPalette = {
  background: Rgb;
  content: Rgb;
  accent: Rgb;
  dark: boolean;
  fontFamily: string;
  fontSize: string;
};

// Series colors for pie, xy and quadrant charts. The accent leads so a
// single-series chart reads as the app's own color.
const SERIES = [
  "#10b981",
  "#e8c547",
  "#f43f5e",
  "#a78bfa",
  "#38bdf8",
  "#fb923c",
  "#94a3b8",
];

const FALLBACK: Record<"light" | "dark", MermaidPalette> = {
  dark: {
    background: [24, 24, 27],
    content: [235, 235, 235],
    accent: [74, 158, 248],
    dark: true,
    fontFamily: "sans-serif",
    fontSize: "14px",
  },
  light: {
    background: [247, 247, 247],
    content: [46, 46, 46],
    accent: [74, 158, 248],
    dark: false,
    fontFamily: "sans-serif",
    fontSize: "14px",
  },
};

function hex([r, g, b]: Rgb): string {
  return `#${[r, g, b]
    .map((value) => Math.round(value).toString(16).padStart(2, "0"))
    .join("")}`;
}

/** `amount` of `top` painted over `base`, as an opaque color. */
function mix(base: Rgb, top: Rgb, amount: number): string {
  return hex([
    base[0] + (top[0] - base[0]) * amount,
    base[1] + (top[1] - base[1]) * amount,
    base[2] + (top[2] - base[2]) * amount,
  ]);
}

/**
 * Mermaid derives shades with its own color math, so it needs concrete
 * colors rather than CSS variables. Every fill is the theme's ink laid
 * over its background at the same strengths the app uses for its own
 * surfaces, which keeps diagrams in step with custom theme hues.
 */
export function mermaidThemeConfig(palette: MermaidPalette): MermaidConfig {
  const { background: bg, content: fg, accent, dark } = palette;
  const surface = mix(bg, fg, 0.06);
  const raised = mix(bg, fg, 0.1);
  const border = mix(bg, fg, 0.28);
  const muted = mix(bg, fg, 0.6);
  const text = hex(fg);
  const series = [hex(accent), ...SERIES];
  const pie = Object.fromEntries(
    series.map((color, index) => [`pie${index + 1}`, color]),
  );
  return {
    theme: "base",
    fontFamily: palette.fontFamily,
    themeVariables: {
      darkMode: dark,
      background: hex(bg),
      fontFamily: palette.fontFamily,
      fontSize: palette.fontSize,
      primaryColor: surface,
      primaryTextColor: text,
      primaryBorderColor: border,
      secondaryColor: raised,
      secondaryTextColor: text,
      secondaryBorderColor: border,
      tertiaryColor: mix(bg, fg, 0.03),
      tertiaryTextColor: text,
      tertiaryBorderColor: border,
      lineColor: muted,
      textColor: text,
      mainBkg: surface,
      nodeBorder: border,
      nodeTextColor: text,
      clusterBkg: mix(bg, fg, 0.03),
      clusterBorder: mix(bg, fg, 0.16),
      titleColor: text,
      edgeLabelBackground: hex(bg),
      noteBkgColor: raised,
      noteTextColor: text,
      noteBorderColor: border,
      actorBkg: surface,
      actorBorder: border,
      actorTextColor: text,
      actorLineColor: muted,
      signalColor: text,
      signalTextColor: text,
      labelBoxBkgColor: surface,
      labelBoxBorderColor: border,
      labelTextColor: text,
      loopTextColor: text,
      activationBkgColor: raised,
      activationBorderColor: border,
      sequenceNumberColor: hex(bg),
      ...pie,
      pieStrokeColor: hex(bg),
      pieOuterStrokeColor: hex(bg),
      pieTitleTextColor: text,
      pieSectionTextColor: dark ? hex(bg) : "#ffffff",
      pieLegendTextColor: text,
      xyChart: {
        backgroundColor: hex(bg),
        titleColor: text,
        xAxisLabelColor: muted,
        xAxisTitleColor: text,
        xAxisTickColor: border,
        xAxisLineColor: border,
        yAxisLabelColor: muted,
        yAxisTitleColor: text,
        yAxisTickColor: border,
        yAxisLineColor: border,
        plotColorPalette: series.join(","),
      },
    },
  };
}

function parseColor(value: string): Rgb | null {
  const short = /^#([\da-f])([\da-f])([\da-f])$/i.exec(value.trim());
  const long = /^#([\da-f]{2})([\da-f]{2})([\da-f]{2})/i.exec(value.trim());
  const hexed = long?.slice(1) ?? short?.slice(1).map((digit) => digit + digit);
  if (hexed) return hexed.map((pair) => parseInt(pair, 16)) as unknown as Rgb;
  const numbers = value.match(/-?[\d.]+/g)?.map(Number);
  if (!numbers || numbers.length < 3) return null;
  // `color(srgb r g b)` reports channels from 0 to 1; `rgb()` from 0 to 255.
  const scale = value.trim().startsWith("color(") ? 255 : 1;
  return [numbers[0] * scale, numbers[1] * scale, numbers[2] * scale];
}

/** Reads the live theme off the document; falls back when it cannot. */
export function readMermaidPalette(scheme: "light" | "dark"): MermaidPalette {
  const fallback = FALLBACK[scheme];
  if (typeof document === "undefined" || !document.body) return fallback;
  const probe = document.createElement("span");
  probe.className = "agent-markdown";
  probe.style.position = "absolute";
  probe.style.visibility = "hidden";
  document.body.appendChild(probe);
  try {
    const read = (color: string) => {
      probe.style.color = color;
      return parseColor(getComputedStyle(probe).color);
    };
    const style = getComputedStyle(probe);
    return {
      background: read("var(--color-background-base)") ?? fallback.background,
      content: read("var(--color-content)") ?? fallback.content,
      accent:
        read("var(--user-accent-color, var(--color-accent))") ??
        fallback.accent,
      dark: scheme === "dark",
      fontFamily: style.fontFamily || fallback.fontFamily,
      fontSize: style.fontSize || fallback.fontSize,
    };
  } finally {
    probe.remove();
  }
}

// Appearance settings write these onto the root element without an event of
// their own; the class carries light/dark and theme presets.
const APPEARANCE_VARIABLES = [
  "--theme-hue",
  "--theme-saturation",
  "--theme-dark-lightness",
  "--user-accent-color",
];

/** Changes whenever the colors a diagram is drawn with change. */
export function mermaidAppearanceKey(): string {
  if (typeof document === "undefined") return "";
  const root = document.documentElement;
  return [
    root.className,
    ...APPEARANCE_VARIABLES.map((name) => root.style.getPropertyValue(name)),
  ].join("|");
}

export function subscribeMermaidAppearance(onChange: () => void): () => void {
  if (typeof MutationObserver === "undefined") return () => {};
  const observer = new MutationObserver(onChange);
  observer.observe(document.documentElement, {
    attributes: true,
    attributeFilter: ["class", "style"],
  });
  return () => observer.disconnect();
}
