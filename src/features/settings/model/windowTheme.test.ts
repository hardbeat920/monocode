// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from "vitest";
import { applyThemePreference, pinPanelWindowTheme } from "./appearance";

const platform = vi.hoisted(() => ({ isMac: true }));
vi.mock("../../../platform/tauri/platform", async (importOriginal) => ({
  ...(await importOriginal<
    typeof import("../../../platform/tauri/platform")
  >()),
  get IS_MAC() {
    return platform.isMac;
  },
}));

const setTheme = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ setTheme }),
}));

beforeEach(() => {
  setTheme.mockReset();
  setTheme.mockResolvedValue(undefined);
  platform.isMac = true;
  localStorage.clear();
  document.documentElement.className = "";
});

describe("window theme", () => {
  it("clears the app-wide window theme when following the system on macOS", () => {
    applyThemePreference("system");
    expect(setTheme).toHaveBeenCalledWith(null);
  });

  it("leaves an explicit theme to the window that pins it", () => {
    applyThemePreference("dark");
    applyThemePreference("light");
    expect(setTheme).not.toHaveBeenCalled();
  });

  it("does not touch the window theme outside macOS", () => {
    platform.isMac = false;
    applyThemePreference("system");
    expect(setTheme).not.toHaveBeenCalled();
  });
});

describe("panel window theme", () => {
  it("pins an explicit theme so the panel's blur matches it", () => {
    pinPanelWindowTheme("dark", "dark");
    pinPanelWindowTheme("light", "light");
    expect(setTheme.mock.calls).toEqual([["dark"], ["light"]]);
  });

  it("never pins the scheme resolved from the system", () => {
    pinPanelWindowTheme("system", "light");
    pinPanelWindowTheme("system", "dark");
    expect(setTheme).not.toHaveBeenCalled();
  });
});
