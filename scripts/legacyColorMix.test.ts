import { describe, expect, it } from "vitest";
import { rewriteColorMix } from "./legacyColorMix";

describe("rewriteColorMix", () => {
  it("turns a mix with transparent into alpha on the resolved theme color", () => {
    const css =
      ":root{--color-content:hsl(var(--theme-hue) var(--theme-saturation) var(--content-lightness))}" +
      ".bg-content\\/10{background-color:color-mix(in oklab,var(--color-content) 10%,transparent)}";
    expect(rewriteColorMix(css)).toContain(
      "background-color:hsl(var(--theme-hue) var(--theme-saturation) var(--content-lightness) / 10%)",
    );
  });

  it("keeps a var() amount", () => {
    expect(
      rewriteColorMix(
        "a{color:color-mix(in srgb, oklch(70% 0.2 20) var(--strength), transparent)}",
      ),
    ).toBe("a{color:oklch(70% 0.2 20 / var(--strength))}");
  });

  it("expands hex colors to rgb with alpha", () => {
    expect(
      rewriteColorMix(
        ":root{--color-accent:#459bf7}a{color:color-mix(in srgb,var(--color-accent) 25%,transparent)}",
      ),
    ).toContain("a{color:rgb(69 155 247 / 25%)}");
  });

  it("mixes two theme colors through their lightness", () => {
    const css =
      ":root{--color-content:hsl(var(--theme-hue) var(--theme-saturation) var(--content-lightness));" +
      "--color-background-base:hsl(var(--theme-hue) var(--theme-saturation) var(--background-lightness))}" +
      "a{color:color-mix(in srgb,var(--color-content) 10%,var(--color-background-base))}";
    expect(rewriteColorMix(css)).toContain(
      "a{color:hsl(var(--theme-hue) var(--theme-saturation) calc(var(--content-lightness) * 0.1 + var(--background-lightness) * 0.9))}",
    );
  });

  it("leaves mixes it cannot resolve untouched", () => {
    const css =
      "a{color:color-mix(in srgb,var(--user-accent-color) 30%,transparent)}";
    expect(rewriteColorMix(css)).toBe(css);
  });
});
