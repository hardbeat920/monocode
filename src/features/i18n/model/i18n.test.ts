import { describe, expect, it, beforeEach } from "vitest";
import {
  loadLanguagePreference,
  saveLanguagePreference,
  resolveLanguage,
  t,
} from "./i18n";

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
  };
  Object.defineProperty(globalThis, "localStorage", {
    value: storage,
    configurable: true,
    writable: true,
  });
}

describe("i18n", () => {
  beforeEach(() => {
    mockLocalStorage();
    localStorage.clear();
  });

  it("defaults to system preference", () => {
    expect(loadLanguagePreference()).toBe("system");
  });

  it("persists language preference", () => {
    saveLanguagePreference("zh-CN");
    expect(loadLanguagePreference()).toBe("zh-CN");
    saveLanguagePreference("en");
    expect(loadLanguagePreference()).toBe("en");
  });

  it("resolves language explicitly", () => {
    expect(resolveLanguage("en")).toBe("en");
    expect(resolveLanguage("zh-CN")).toBe("zh-CN");
  });

  it("translates known keys in zh-CN", () => {
    saveLanguagePreference("zh-CN");
    expect(t("common.save", "Save")).toBe("保存");
    expect(t("nav.settings", "Settings")).toBe("设置");
    expect(t("settings.general.title", "General")).toBe("常规");
  });

  it("falls back to English when in en mode", () => {
    saveLanguagePreference("en");
    expect(t("common.save", "Save")).toBe("Save");
    expect(t("nav.settings", "Settings")).toBe("Settings");
  });

  it("falls back gracefully for missing keys", () => {
    expect(t("non.existent.key", "Custom Fallback")).toBe("Custom Fallback");
  });
});
