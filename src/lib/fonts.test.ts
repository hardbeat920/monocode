import { describe, expect, it, beforeEach } from "vitest";
import {
  CODE_FONT_SIZE_DEFAULT,
  CODE_FONT_SIZE_MAX,
  CODE_FONT_SIZE_MIN,
  UI_FONT_WEIGHT_DEFAULT,
  codeFontStack,
  loadCodeFontSize,
  loadCodeFontFamily,
  loadCodeFontWeight,
  loadUiFontFamily,
  loadUiFontWeight,
  normalizeCodeFontSize,
  normalizeFontFamily,
  normalizeFontWeight,
  saveCodeFontFamily,
  saveCodeFontSize,
  saveCodeFontWeight,
  saveUiFontFamily,
  saveUiFontWeight,
  uiFontStack,
} from "./fonts";

function mockLocalStorage() {
  const data = new Map<string, string>();
  const storage = {
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => {
      data.set(key, value);
    },
    removeItem: (key: string) => {
      data.delete(key);
    },
    clear: () => {
      data.clear();
    },
    key: (index: number) => [...data.keys()][index] ?? null,
    get length() {
      return data.size;
    },
  };
  Object.defineProperty(globalThis, "localStorage", {
    value: storage,
    configurable: true,
  });
}

describe("font family settings", () => {
  beforeEach(mockLocalStorage);

  it("defaults to the system stacks", () => {
    expect(loadUiFontFamily()).toBe("");
    expect(loadCodeFontFamily()).toBe("");
    expect(uiFontStack("")).toContain("system-ui");
    expect(codeFontStack("")).toContain("ui-monospace");
  });

  it("prefixes a picked family ahead of the fallback", () => {
    expect(uiFontStack("Inter")).toBe(
      `"Inter", system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Oxygen, Ubuntu, Cantarell, "Fira Sans", "Droid Sans", "Helvetica Neue", sans-serif`,
    );
    expect(codeFontStack("JetBrains Mono")).toContain('"JetBrains Mono"');
    expect(codeFontStack("JetBrains Mono")).toContain("ui-monospace");
  });

  it("escapes quotes in family names for the CSS stack", () => {
    const stack = codeFontStack('Evil "Font"');
    expect(stack).toContain('Evil \\"Font\\"');
  });

  it("trims and drops control characters", () => {
    expect(normalizeFontFamily("  Inter\n")).toBe("Inter");
    expect(normalizeFontFamily(42)).toBe("");
  });

  it("persists and clears the picked families", () => {
    saveUiFontFamily("Inter");
    expect(loadUiFontFamily()).toBe("Inter");
    saveUiFontFamily("");
    expect(loadUiFontFamily()).toBe("");
    saveCodeFontFamily("JetBrains Mono");
    expect(loadCodeFontFamily()).toBe("JetBrains Mono");
    saveCodeFontFamily("   ");
    expect(loadCodeFontFamily()).toBe("");
  });
});

describe("code size / weight settings", () => {
  beforeEach(mockLocalStorage);

  it("defaults to the current editor metrics", () => {
    expect(loadCodeFontSize()).toBe(CODE_FONT_SIZE_DEFAULT);
    expect(loadUiFontWeight()).toBe(UI_FONT_WEIGHT_DEFAULT);
    expect(loadCodeFontWeight()).toBe(400);
  });

  it("clamps size into range", () => {
    expect(normalizeCodeFontSize(4)).toBe(CODE_FONT_SIZE_MIN);
    expect(normalizeCodeFontSize(99)).toBe(CODE_FONT_SIZE_MAX);
    expect(normalizeCodeFontSize("junk")).toBe(CODE_FONT_SIZE_DEFAULT);
    saveCodeFontSize(15);
    expect(loadCodeFontSize()).toBe(15);
  });

  it("snaps weights to hundreds inside 400-700", () => {
    expect(normalizeFontWeight(450, 400)).toBe(500);
    expect(normalizeFontWeight(100, 400)).toBe(400);
    expect(normalizeFontWeight(900, 400)).toBe(700);
    expect(normalizeFontWeight("junk", 400)).toBe(400);
    saveUiFontWeight(600);
    expect(loadUiFontWeight()).toBe(600);
    saveCodeFontWeight(500);
    expect(loadCodeFontWeight()).toBe(500);
  });
});
