// @vitest-environment happy-dom
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import {
  applyFonts,
  fontFamilyCss,
  initFonts,
  loadFontFamily,
  saveFontFamily,
} from "./fonts";

beforeEach(() => {
  const data = new Map<string, string>();
  vi.stubGlobal("localStorage", {
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => data.set(key, value),
    removeItem: (key: string) => data.delete(key),
  });
  document.documentElement.removeAttribute("style");
});
afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

it("persists independent UI and code fonts and restores defaults without clearing the other", () => {
  saveFontFamily("ui", "Avenir Next");
  saveFontFamily("code", "Menlo");
  expect(loadFontFamily("ui")).toBe("Avenir Next");
  expect(loadFontFamily("code")).toBe("Menlo");
  applyFonts();
  expect(
    document.documentElement.style.getPropertyValue("--font-sans"),
  ).toContain('"Avenir Next"');
  expect(
    document.documentElement.style.getPropertyValue("--font-mono"),
  ).toContain('"Menlo"');
  saveFontFamily("ui", "");
  expect(document.documentElement.style.getPropertyValue("--font-sans")).toBe(
    "",
  );
  expect(loadFontFamily("code")).toBe("Menlo");
});

it("quotes font names as literal families, with a fallback for unavailable fonts", () => {
  expect(fontFamilyCss("code", 'Font "One", Two\\Three')).toBe(
    '"Font \\"One\\", Two\\\\Three", var(--font-mono-default)',
  );
  expect(fontFamilyCss("ui", "")).toBe("var(--font-sans-default)");
});

it("retains system defaults when storage cannot be read", () => {
  vi.spyOn(localStorage, "getItem").mockImplementation(() => {
    throw new Error("denied");
  });
  expect(loadFontFamily("ui")).toBe("");
  expect(loadFontFamily("code")).toBe("");
});

it("loads saved fonts at startup and follows font changes from another window", () => {
  localStorage.setItem("monocode.uiFontFamily", "Georgia");
  initFonts();
  expect(
    document.documentElement.style.getPropertyValue("--font-sans"),
  ).toContain('"Georgia"');
  localStorage.setItem("monocode.codeFontFamily", "Menlo");
  window.dispatchEvent(
    new StorageEvent("storage", { key: "monocode.codeFontFamily" }),
  );
  expect(
    document.documentElement.style.getPropertyValue("--font-mono"),
  ).toContain('"Menlo"');
  expect(
    document.documentElement.style.getPropertyValue("--font-sans"),
  ).toContain('"Georgia"');
});
