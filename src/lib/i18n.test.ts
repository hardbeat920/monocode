import { describe, it, expect, beforeEach } from "vitest";
import { t, setLocale, getLocale } from "./i18n";

describe("i18n", () => {
  beforeEach(() => {
    setLocale("zh-CN");
  });

  it("returns Chinese translation for a known key", () => {
    expect(t("Close")).toBe("关闭");
  });

  it("returns the key itself for a missing key", () => {
    expect(t("nonexistent key")).toBe("nonexistent key");
  });

  it("replaces a single positional placeholder", () => {
    expect(t("{0} sessions", ["3"])).toBe("3 个会话");
  });

  it("replaces multiple positional placeholders", () => {
    expect(t("{0} — {1} — MonoCode", ["a", "b"])).toBe("a — b — MonoCode");
  });

  it("translates a key with no placeholders", () => {
    expect(t("Settings")).toBe("设置");
  });

  it("translates correctly when params is an empty array", () => {
    expect(t("Close", [])).toBe("关闭");
  });

  it("setLocale changes the active locale", () => {
    setLocale("en-US");
    expect(getLocale()).toBe("en-US");
  });

  it("getLocale returns the current locale", () => {
    expect(getLocale()).toBe("zh-CN");
    setLocale("fr-FR");
    expect(getLocale()).toBe("fr-FR");
  });
});
