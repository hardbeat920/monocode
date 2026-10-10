// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  mermaidAppearanceKey,
  mermaidThemeConfig,
  readMermaidPalette,
  subscribeMermaidAppearance,
} from "./mermaidTheme";

afterEach(() => {
  for (const name of ["--color-accent", "--user-accent-color", "--theme-hue"])
    document.documentElement.style.removeProperty(name);
});

const palette = {
  background: [0, 0, 0] as const,
  content: [255, 255, 255] as const,
  accent: [0, 128, 255] as const,
  dark: true,
  fontFamily: "Inter, sans-serif",
  fontSize: "14px",
};

describe("mermaidThemeConfig", () => {
  it("draws with the app's ink, background and type", () => {
    const config = mermaidThemeConfig(palette);
    const vars = config.themeVariables as Record<string, unknown>;

    expect(config.theme).toBe("base");
    expect(config.fontFamily).toBe("Inter, sans-serif");
    expect(vars.background).toBe("#000000");
    expect(vars.primaryTextColor).toBe("#ffffff");
    expect(vars.primaryColor).toBe("#0f0f0f");
    expect(vars.primaryBorderColor).toBe("#474747");
    expect(vars.darkMode).toBe(true);
  });

  it("leads chart series with the accent", () => {
    const vars = mermaidThemeConfig(palette).themeVariables as Record<
      string,
      unknown
    >;

    expect(vars.pie1).toBe("#0080ff");
    expect(
      (vars.xyChart as { plotColorPalette: string }).plotColorPalette,
    ).toMatch(/^#0080ff,/);
  });
});

describe("readMermaidPalette", () => {
  it("falls back to the scheme's defaults when the theme cannot be read", () => {
    const light = readMermaidPalette("light");

    expect(light.dark).toBe(false);
    expect(light.background).toHaveLength(3);
    expect(light.content).toHaveLength(3);
  });
});

describe("readMermaidPalette accent", () => {
  it("uses the custom accent when one is set", () => {
    document.documentElement.style.setProperty("--color-accent", "#0000ff");
    document.documentElement.style.setProperty(
      "--user-accent-color",
      "#ff0000",
    );

    expect(readMermaidPalette("dark").accent).toEqual([255, 0, 0]);
  });

  it("falls back to the theme accent without a custom one", () => {
    document.documentElement.style.setProperty("--color-accent", "#0000ff");

    expect(readMermaidPalette("dark").accent).toEqual([0, 0, 255]);
  });
});

describe("mermaid appearance", () => {
  it("changes its key and notifies when a theme color changes", async () => {
    const onChange = vi.fn();
    const unsubscribe = subscribeMermaidAppearance(onChange);
    const before = mermaidAppearanceKey();

    document.documentElement.style.setProperty("--theme-hue", "120");
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(onChange).toHaveBeenCalled();
    expect(mermaidAppearanceKey()).not.toBe(before);
    unsubscribe();
  });
});

describe("pie section labels", () => {
  function channels(color: string): number[] {
    return [1, 3, 5].map((index) =>
      parseInt(color.slice(index, index + 2), 16),
    );
  }
  function contrast(a: string, b: string): number {
    const luminance = (color: string) => {
      const [r, g, b] = channels(color).map((value) => {
        const channel = value / 255;
        return channel <= 0.03928
          ? channel / 12.92
          : ((channel + 0.055) / 1.055) ** 2.4;
      });
      return 0.2126 * r + 0.7152 * g + 0.0722 * b;
    };
    const [light, dark] = [luminance(a), luminance(b)].sort((x, y) => y - x);
    return (light + 0.05) / (dark + 0.05);
  }

  const themes = {
    light: { background: [247, 247, 247], content: [46, 46, 46], dark: false },
    dark: { background: [24, 24, 27], content: [235, 235, 235], dark: true },
  } as const;
  const accents = {
    blue: [74, 158, 248],
    navy: [30, 58, 138],
    yellow: [232, 197, 71],
    white: [255, 255, 255],
  } as const;

  for (const [scheme, theme] of Object.entries(themes)) {
    for (const [name, accent] of Object.entries(accents)) {
      it(`keeps every slice at 4.5:1 in ${scheme} mode with a ${name} accent`, () => {
        const vars = mermaidThemeConfig({
          ...theme,
          accent,
          fontFamily: "sans-serif",
          fontSize: "14px",
        }).themeVariables as Record<string, string>;
        const fills = Object.keys(vars)
          .filter((key) => /^pie\d+$/.test(key))
          .map((key) => vars[key]);

        expect(fills).toHaveLength(8);
        for (const fill of fills) {
          expect(
            contrast(fill, vars.pieSectionTextColor),
          ).toBeGreaterThanOrEqual(4.5);
        }
      });
    }
  }
});
