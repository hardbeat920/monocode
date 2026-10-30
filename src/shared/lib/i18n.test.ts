import { afterEach, beforeEach, describe, expect, it } from "vitest";
import {
  currentLanguage,
  LANGUAGE_CHANGE_EVENT,
  LANGUAGE_DEFAULT,
  loadLanguage,
  resolveLanguage,
  saveLanguage,
  subscribeLanguage,
  t,
} from "./i18n";

const KEY = "monocode.language";

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

describe("language preference", () => {
  beforeEach(mockLocalStorage);

  it("defaults to auto", () => {
    expect(LANGUAGE_DEFAULT).toBe("auto");
    expect(loadLanguage()).toBe("auto");
  });

  it("persists an explicit language", () => {
    saveLanguage("zh");
    expect(localStorage.getItem(KEY)).toBe("zh");
    expect(loadLanguage()).toBe("zh");
  });

  it("ignores unknown stored values", () => {
    localStorage.setItem(KEY, "fr");
    expect(loadLanguage()).toBe("auto");
  });
});

describe("resolveLanguage", () => {
  afterEach(() => {
    Reflect.deleteProperty(globalThis, "navigator");
  });

  it("passes explicit choices through", () => {
    expect(resolveLanguage("en")).toBe("en");
    expect(resolveLanguage("zh")).toBe("zh");
  });

  it("follows the system language for auto", () => {
    Object.defineProperty(globalThis, "navigator", {
      value: { language: "zh-CN" },
      configurable: true,
    });
    expect(resolveLanguage("auto")).toBe("zh");

    Object.defineProperty(globalThis, "navigator", {
      value: { language: "de-DE" },
      configurable: true,
    });
    expect(resolveLanguage("auto")).toBe("en");
  });

  it("falls back to english when there is no navigator", () => {
    Reflect.deleteProperty(globalThis, "navigator");
    expect(resolveLanguage("auto")).toBe("en");
  });
});

describe("t", () => {
  beforeEach(mockLocalStorage);
  afterEach(() => {
    localStorage.removeItem(KEY);
  });

  it("returns the key itself when the language is english", () => {
    saveLanguage("en");
    expect(currentLanguage()).toBe("en");
    expect(t("Settings")).toBe("Settings");
    expect(t("No project")).toBe("No project");
  });

  it("returns the translation when the language is chinese", () => {
    saveLanguage("zh");
    expect(currentLanguage()).toBe("zh");
    expect(t("Settings")).toBe("设置");
    expect(t("No project")).toBe("无项目");
  });

  it("interpolates named parameters", () => {
    saveLanguage("zh");
    expect(t("{count} sessions", { count: 3 })).toBe("3 个会话");
    expect(t("Archived in {name}", { name: "demo" })).toBe("demo 的归档");
  });

  it("keeps the source text when a key is missing from the table", () => {
    saveLanguage("zh");
    expect(t("Not a translated key")).toBe("Not a translated key");
  });

  it("leaves unknown placeholders alone", () => {
    saveLanguage("zh");
    expect(t("{count} sessions")).toBe("{count} 个会话");
  });
});

/** Minimal EventTarget stand-in: `window` is absent under vitest's node env. */
function mockWindow() {
  const listeners = new Map<string, Set<() => void>>();
  Object.defineProperty(globalThis, "window", {
    value: {
      addEventListener: (type: string, listener: () => void) => {
        const bucket = listeners.get(type) ?? new Set();
        bucket.add(listener);
        listeners.set(type, bucket);
      },
      removeEventListener: (type: string, listener: () => void) => {
        listeners.get(type)?.delete(listener);
      },
      dispatchEvent: (event: { type: string }) => {
        for (const listener of listeners.get(event.type) ?? []) listener();
        return true;
      },
    },
    configurable: true,
  });
}

describe("language change events", () => {
  beforeEach(() => {
    mockLocalStorage();
    mockWindow();
  });
  afterEach(() => {
    localStorage.removeItem(KEY);
    Reflect.deleteProperty(globalThis, "window");
  });

  it("notifies subscribers and re-reads the table", () => {
    saveLanguage("en");
    let calls = 0;
    const unsubscribe = subscribeLanguage(() => {
      calls += 1;
    });
    saveLanguage("zh");
    expect(calls).toBe(1);
    expect(t("Settings")).toBe("设置");
    unsubscribe();
    saveLanguage("en");
    expect(calls).toBe(1);
    expect(t("Settings")).toBe("Settings");
  });

  it("re-reads the table when localStorage changes behind its back", () => {
    saveLanguage("en");
    expect(t("Settings")).toBe("Settings");
    // Another window (or a test) writes the key directly: no event fires.
    localStorage.setItem(KEY, "zh");
    expect(t("Settings")).toBe("设置");
  });

  it("uses the documented event name", () => {
    expect(LANGUAGE_CHANGE_EVENT).toBe("monocode:language-change");
  });
});
