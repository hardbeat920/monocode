// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from "vitest";
import { syncNativeGlass } from "./appearance";

const platform = vi.hoisted(() => ({ isLinux: false }));
vi.mock("../../../platform/tauri/platform", async (importOriginal) => ({
  ...(await importOriginal<
    typeof import("../../../platform/tauri/platform")
  >()),
  get IS_LINUX() {
    return platform.isLinux;
  },
}));

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tauri-apps/api/core")>()),
  invoke,
}));

const BODY_GLASS_KEY = "monocode.bodyGlass";

function hasGlass() {
  return document.documentElement.classList.contains("has-native-glass");
}

beforeEach(() => {
  invoke.mockReset();
  invoke.mockResolvedValue(undefined);
  platform.isLinux = false;
  localStorage.clear();
  document.documentElement.className = "";
  document.documentElement.style.cssText = "";
});

describe("native glass", () => {
  it("turns glass on and marks the page translucent in dark mode", async () => {
    syncNativeGlass("dark");
    await vi.waitFor(() => expect(hasGlass()).toBe(true));
    expect(invoke).toHaveBeenCalledWith(
      "set_window_glass_enabled",
      expect.objectContaining({ enabled: true }),
    );
  });

  it("turns glass off and paints the page opaque in light mode", async () => {
    syncNativeGlass("light");
    await vi.waitFor(() => expect(hasGlass()).toBe(false));
    expect(invoke).toHaveBeenCalledWith("set_window_glass_enabled", {
      enabled: false,
      background: expect.any(Object),
    });
  });

  it("fills an opaque window with the themed colour, not a fixed light one", async () => {
    document.documentElement.style.setProperty("--background-lightness", "20%");
    syncNativeGlass("light");
    await vi.waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("set_window_glass_enabled", {
        enabled: false,
        background: { r: 51, g: 51, b: 51 },
      }),
    );
  });

  it("keeps the dark theme opaque on Linux when main pane glass is off", async () => {
    platform.isLinux = true;
    localStorage.setItem(BODY_GLASS_KEY, "0");
    syncNativeGlass("dark");
    await vi.waitFor(() => expect(hasGlass()).toBe(false));
    expect(invoke).toHaveBeenCalledWith("set_window_glass_enabled", {
      enabled: false,
      background: { r: 23, g: 23, b: 23 },
    });
  });

  it("enables glass on Linux once main pane glass is on", async () => {
    platform.isLinux = true;
    localStorage.setItem(BODY_GLASS_KEY, "1");
    syncNativeGlass("dark");
    await vi.waitFor(() => expect(hasGlass()).toBe(true));
    expect(invoke).toHaveBeenCalledWith(
      "set_window_glass_enabled",
      expect.objectContaining({ enabled: true }),
    );
  });

  it("waits for the window to settle before the page changes", async () => {
    let settle = () => {};
    invoke.mockReturnValue(
      new Promise<void>((resolve) => {
        settle = resolve;
      }),
    );
    syncNativeGlass("dark");
    await Promise.resolve();
    expect(hasGlass()).toBe(false);
    settle();
    await vi.waitFor(() => expect(hasGlass()).toBe(true));
  });

  it("still flips the page when the window call fails", async () => {
    invoke.mockRejectedValue(new Error("no window"));
    syncNativeGlass("light");
    await vi.waitFor(() => expect(hasGlass()).toBe(false));
    invoke.mockRejectedValue(new Error("no window"));
    syncNativeGlass("dark");
    await vi.waitFor(() => expect(hasGlass()).toBe(true));
  });
});
