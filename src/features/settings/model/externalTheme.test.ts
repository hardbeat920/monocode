// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from "vitest";
import { hexToHsl } from "../../../shared/lib/colorUtils";
import {
  loadAccentColor,
  loadThemeDarkLightness,
  loadThemeHue,
  loadThemePreference,
  loadThemeSaturation,
  saveThemeDarkLightness,
} from "./appearance";
import {
  applyExternalThemeSource,
  EXTERNAL_THEME_CHANGED_EVENT,
  initExternalTheme,
  parseExternalTheme,
} from "./externalTheme";

const tauri = vi.hoisted(() => ({
  invoke: vi.fn((): Promise<unknown> => Promise.resolve()),
  listeners: new Map<string, (event: { payload: unknown }) => void>(),
}));
vi.mock("@tauri-apps/api/core", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tauri-apps/api/core")>()),
  invoke: tauri.invoke,
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(
    (name: string, handler: (event: { payload: unknown }) => void) => {
      tauri.listeners.set(name, handler);
      return Promise.resolve(() => {});
    },
  ),
}));

const LATTE = JSON.stringify({
  appearance: "light",
  background: "#eff1f5",
  accent: "#1e66f5",
});

const OSAKA_JADE = JSON.stringify({
  appearance: "dark",
  background: "#111c18",
  accent: "#509475",
});

function mockLocalStorage() {
  const data = new Map<string, string>();
  Object.defineProperty(globalThis, "localStorage", {
    value: {
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
    },
    configurable: true,
  });
}

beforeEach(() => {
  mockLocalStorage();
  document.documentElement.className = "";
  document.documentElement.removeAttribute("style");
});

describe("hexToHsl", () => {
  it("inverts the colour the theme renders", () => {
    expect(hexToHsl("#111c18")).toEqual({
      h: expect.closeTo(158.18, 1),
      s: expect.closeTo(24.44, 1),
      l: expect.closeTo(8.82, 1),
    });
    expect(hexToHsl("#808080")).toEqual({
      h: 0,
      s: 0,
      l: expect.closeTo(50.2, 1),
    });
  });
});

describe("parseExternalTheme", () => {
  it("keeps only valid fields", () => {
    expect(
      parseExternalTheme(
        JSON.stringify({
          appearance: "dusk",
          background: "#ABCDEF",
          accent: "teal",
          extra: true,
        }),
      ),
    ).toEqual({ background: "#abcdef" });
  });

  it("rejects input that is not a JSON object", () => {
    expect(parseExternalTheme("{")).toBeNull();
    expect(parseExternalTheme("[]")).toBeNull();
    expect(parseExternalTheme("null")).toBeNull();
  });
});

describe("applyExternalThemeSource", () => {
  it("saves and applies every field", () => {
    applyExternalThemeSource(OSAKA_JADE, { onlyIfNew: true });

    expect(loadThemeHue()).toBe(158);
    expect(loadThemeSaturation()).toBe(24);
    expect(loadThemeDarkLightness()).toBe(9);
    expect(loadAccentColor()).toBe("#509475");
    expect(loadThemePreference()).toBe("dark");
    const style = document.documentElement.style;
    expect(style.getPropertyValue("--theme-hue")).toBe("158");
    expect(style.getPropertyValue("--user-accent-color")).toBe("#509475");
  });

  it("keeps the dark lightness for a light theme", () => {
    saveThemeDarkLightness(12);
    applyExternalThemeSource(
      JSON.stringify({ appearance: "light", background: "#fafafa" }),
      { onlyIfNew: true },
    );

    expect(loadThemeDarkLightness()).toBe(12);
    expect(loadThemePreference()).toBe("light");
    expect(document.documentElement.classList.contains("theme-light")).toBe(
      true,
    );
  });

  it("leaves manual changes alone at launch until the file changes", () => {
    applyExternalThemeSource(OSAKA_JADE, { onlyIfNew: true });
    saveThemeDarkLightness(20);

    applyExternalThemeSource(OSAKA_JADE, { onlyIfNew: true });
    expect(loadThemeDarkLightness()).toBe(20);

    applyExternalThemeSource(OSAKA_JADE, { onlyIfNew: false });
    expect(loadThemeDarkLightness()).toBe(9);
  });

  it("repaints a window that loaded settings before another window applied the file", () => {
    applyExternalThemeSource(OSAKA_JADE, { onlyIfNew: true });
    // This window painted the defaults before the other one saved the file.
    document.documentElement.style.setProperty("--theme-hue", "240");
    document.documentElement.classList.add("theme-light");

    applyExternalThemeSource(OSAKA_JADE, { onlyIfNew: true });

    const root = document.documentElement;
    expect(root.style.getPropertyValue("--theme-hue")).toBe("158");
    expect(root.classList.contains("theme-light")).toBe(false);
  });

  it("ignores a missing or unreadable file", () => {
    applyExternalThemeSource(null, { onlyIfNew: false });
    applyExternalThemeSource("not json", { onlyIfNew: false });
    expect(localStorage.length).toBe(0);
  });
});

describe("initExternalTheme", () => {
  it("keeps a change heard while the launch read was in flight", async () => {
    let finishRead: (raw: string) => void = () => {};
    tauri.invoke.mockImplementationOnce(
      () => new Promise((resolve) => (finishRead = resolve)),
    );

    const started = initExternalTheme();
    await vi.waitFor(() => expect(tauri.invoke).toHaveBeenCalled());
    tauri.listeners.get(EXTERNAL_THEME_CHANGED_EVENT)?.({ payload: LATTE });
    finishRead(OSAKA_JADE);
    await started;

    expect(loadThemePreference()).toBe("light");
    expect(loadAccentColor()).toBe("#1e66f5");
  });
});
