import { describe, expect, it } from "vitest";
import {
  NERD_FONT_FALLBACKS,
  splitFontFamilies,
  terminalFontFamily,
} from "./terminalFont";

describe("splitFontFamilies", () => {
  it("splits on top-level commas only", () => {
    expect(splitFontFamilies(` Menlo, "Foo, Bar", 'Baz' ,`)).toEqual([
      "Menlo",
      '"Foo, Bar"',
      "'Baz'",
    ]);
  });
});

describe("terminalFontFamily", () => {
  it("adds Nerd Font fallbacks before the generic monospace", () => {
    const families = splitFontFamilies(
      terminalFontFamily("", "ui-monospace, Menlo, monospace"),
    );
    expect(families[0]).toBe("ui-monospace");
    expect(families).toEqual(
      expect.arrayContaining(NERD_FONT_FALLBACKS),
    );
    expect(families.at(-1)).toBe("monospace");
    expect(families.filter((f) => f === "monospace")).toHaveLength(1);
  });

  it("puts the custom font first and quotes names with spaces", () => {
    const families = splitFontFamilies(
      terminalFontFamily("MesloLGS NF", "Menlo, monospace"),
    );
    expect(families[0]).toBe('"MesloLGS NF"');
    expect(families.filter((f) => /MesloLGS NF/.test(f))).toHaveLength(1);
  });

  it("keeps already-quoted and multi-family custom values", () => {
    const families = splitFontFamilies(
      terminalFontFamily(`'Hack Nerd Font', Menlo`, "Menlo, monospace"),
    );
    expect(families.slice(0, 2)).toEqual(["'Hack Nerd Font'", "Menlo"]);
  });

  it("falls back to the built-in stack without CSS", () => {
    expect(terminalFontFamily("", "")).toMatch(/^ui-monospace, /);
  });
});
