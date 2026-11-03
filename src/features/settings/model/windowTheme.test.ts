// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from "vitest";
import { applyThemePreference } from "./appearance";

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
