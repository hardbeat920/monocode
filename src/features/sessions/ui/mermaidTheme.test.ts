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
