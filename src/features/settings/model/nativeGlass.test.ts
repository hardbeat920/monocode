// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
let appearance: typeof import("./appearance");
const syncNativeGlass = (scheme: "dark" | "light") => appearance.syncNativeGlass(scheme);

const platform = vi.hoisted(() => ({ isLinux: false, isMac: false }));
vi.mock("../../../platform/tauri/platform", async (importOriginal) => ({
  ...(await importOriginal<
    typeof import("../../../platform/tauri/platform")
  >()),
  get IS_MAC() {
    return platform.isMac;
  },
  get IS_LINUX() {
    return platform.isLinux;
  },
}));

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tauri-apps/api/core")>()),
  invoke,
}));

const nativeWindow = vi.hoisted(() => ({ listen: vi.fn(), stop: vi.fn() }));
vi.mock("@tauri-apps/api/webviewWindow", () => ({
  getCurrentWebviewWindow: () => nativeWindow,
}));
let fullscreenEvent: (event: { payload: { fullscreen: boolean; revision: number } }) => void;
const emitFullscreen = (fullscreen: boolean, revision: number) =>
  fullscreenEvent({ payload: { fullscreen, revision } });

const BODY_GLASS_KEY = "monocode.bodyGlass";

function hasGlass() {
  return document.documentElement.classList.contains("has-native-glass");
}

beforeEach(async () => {
  vi.resetModules();
  appearance = await import("./appearance");
  nativeWindow.stop.mockReset();
  nativeWindow.listen.mockReset();
  nativeWindow.listen.mockImplementation((_name, callback) => {
    fullscreenEvent = callback;
    return Promise.resolve(nativeWindow.stop);
  });
  invoke.mockReset();
  invoke.mockResolvedValue(undefined);
  platform.isLinux = false;
  platform.isMac = false;
  localStorage.clear();
  document.documentElement.className = "";
  document.documentElement.style.cssText = "";
  // The real token, so the tests below exercise the deferred window call rather
  // than an unparseable duration that would collapse it to zero.
  document.documentElement.style.setProperty(
    "--motion-feedback-duration",
    "120ms",
  );
});

afterEach(() => appearance.disposeWindowAppearance());

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
    expect(hasGlass()).toBe(false);
    await vi.waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("set_window_glass_enabled", {
        enabled: false,
        background: expect.any(Object),
      }),
    );
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
    expect(hasGlass()).toBe(false);
    await vi.waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("set_window_glass_enabled", {
        enabled: false,
        background: { r: 23, g: 23, b: 23 },
      }),
    );
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

  it("fades the page opaque before the window stops being transparent", async () => {
    platform.isLinux = true;
    localStorage.setItem(BODY_GLASS_KEY, "1");
    syncNativeGlass("dark");
    await vi.waitFor(() => expect(hasGlass()).toBe(true));

    invoke.mockClear();
    localStorage.setItem(BODY_GLASS_KEY, "0");
    syncNativeGlass("dark");
    expect(hasGlass()).toBe(false);
    expect(invoke).not.toHaveBeenCalled();
    await vi.waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("set_window_glass_enabled", {
        enabled: false,
        background: { r: 23, g: 23, b: 23 },
      }),
    );
  });

  it("drops a fade still owed to glass that is back on", async () => {
    platform.isLinux = true;
    localStorage.setItem(BODY_GLASS_KEY, "1");
    syncNativeGlass("dark");
    await vi.waitFor(() => expect(hasGlass()).toBe(true));

    invoke.mockClear();
    localStorage.setItem(BODY_GLASS_KEY, "0");
    syncNativeGlass("dark");
    localStorage.setItem(BODY_GLASS_KEY, "1");
    syncNativeGlass("dark");
    await vi.waitFor(() => expect(hasGlass()).toBe(true));
    await new Promise((resolve) => setTimeout(resolve, 250));
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith(
      "set_window_glass_enabled",
      expect.objectContaining({ enabled: true }),
    );
  });

  it("ignores an enable that a newer disable has overtaken", async () => {
    let settle = () => {};
    invoke.mockReturnValue(
      new Promise<void>((resolve) => {
        settle = resolve;
      }),
    );

    platform.isLinux = true;
    localStorage.setItem(BODY_GLASS_KEY, "1");
    syncNativeGlass("dark");
    localStorage.setItem(BODY_GLASS_KEY, "0");
    syncNativeGlass("dark");
    expect(hasGlass()).toBe(false);

    settle();
    await new Promise((resolve) => setTimeout(resolve, 250));
    expect(hasGlass()).toBe(false);
    expect(invoke).toHaveBeenLastCalledWith(
      "set_window_glass_enabled",
      expect.objectContaining({ enabled: false }),
    );
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

describe("macOS fullscreen appearance", () => {
  beforeEach(() => { platform.isMac = true; });

  async function activate() {
    appearance.activateWindowAppearance();
    await vi.waitFor(() => expect(nativeWindow.listen).toHaveBeenCalledWith(
      "macos-fullscreen-appearance", expect.any(Function),
    ));
    await vi.waitFor(() => expect(hasGlass()).toBe(true));
  }

  it("uses an opaque themed fallback for initial/restored fullscreen", async () => {
    invoke.mockResolvedValue({ fullscreen: true, revision: 0 });
    appearance.activateWindowAppearance();
    await vi.waitFor(() => expect(document.documentElement.classList.contains("macos-fullscreen")).toBe(true));
    expect(hasGlass()).toBe(false);
    expect(invoke).toHaveBeenLastCalledWith("set_window_glass_enabled", {
      enabled: true, background: { r: 23, g: 23, b: 23 }, generation: expect.any(Number),
    });
  });

  it("restores glass after exit without changing opacity, blur or main-pane preferences", async () => {
    localStorage.setItem(BODY_GLASS_KEY, "0");
    appearance.saveSidebarOpacity(0.35);
    appearance.saveSidebarBlur(42);
    appearance.applySidebarOpacity(0.35);
    appearance.applyBodyGlass(false);
    await activate();
    emitFullscreen(true, 1);
    expect(hasGlass()).toBe(false);
    expect(appearance.loadSidebarOpacity()).toBe(0.35);
    expect(appearance.loadSidebarBlur()).toBe(42);
    expect(appearance.loadBodyGlass()).toBe(false);
    appearance.applyBodyGlass(true);
    appearance.saveBodyGlass(true);
    appearance.applySidebarOpacity(0.6);
    appearance.saveSidebarOpacity(0.6);
    appearance.applySidebarBlur(12);
    appearance.saveSidebarBlur(12);
    emitFullscreen(false, 2);
    await vi.waitFor(() => expect(hasGlass()).toBe(true));
    expect(appearance.loadSidebarOpacity()).toBe(0.6);
    expect(appearance.loadSidebarBlur()).toBe(12);
    expect(appearance.loadBodyGlass()).toBe(true);
    expect(document.documentElement.style.getPropertyValue("--sidebar-opacity")).toBe("0.6");
    expect(document.documentElement.classList.contains("glass-body")).toBe(true);
  });

  it("updates the current themed fallback while fullscreen and restores the latest theme", async () => {
    await activate();
    emitFullscreen(true, 1);
    document.documentElement.style.setProperty("--background-lightness", "20%");
    appearance.applyThemeTint(0, 100);
    expect(invoke).toHaveBeenLastCalledWith("set_window_glass_enabled", expect.objectContaining({
      enabled: true, background: { r: 102, g: 0, b: 0 },
    }));
    appearance.applyThemePreference("light");
    expect(hasGlass()).toBe(false);
    expect(invoke).toHaveBeenLastCalledWith("set_window_glass_enabled", expect.objectContaining({ enabled: false }));
    emitFullscreen(false, 2);
    expect(hasGlass()).toBe(false);
    appearance.applyThemePreference("dark");
    await vi.waitFor(() => expect(hasGlass()).toBe(true));
  });

  it("ignores pending enables and stale native snapshots after a newer transition", async () => {
    await activate();
    let settle!: (state: { fullscreen: boolean; revision: number }) => void;
    invoke.mockReturnValueOnce(new Promise((resolve) => { settle = resolve; }));
    syncNativeGlass("dark");
    emitFullscreen(true, 4);
    settle({ fullscreen: false, revision: 3 });
    await Promise.resolve();
    await Promise.resolve();
    expect(hasGlass()).toBe(false);
    expect(document.documentElement.classList.contains("macos-fullscreen")).toBe(true);
    emitFullscreen(false, 5);
    emitFullscreen(true, 6);
    emitFullscreen(false, 7);
    await vi.waitFor(() => expect(hasGlass()).toBe(true));
    emitFullscreen(true, 6);
    expect(document.documentElement.classList.contains("macos-fullscreen")).toBe(false);
  });

  it("removes listeners and invalidates pending completions on destruction", async () => {
    await activate();
    let settle!: () => void;
    invoke.mockReturnValueOnce(new Promise<void>((resolve) => { settle = resolve; }));
    syncNativeGlass("dark");
    document.documentElement.classList.remove("has-native-glass");
    window.dispatchEvent(new Event("pagehide"));
    expect(nativeWindow.stop).toHaveBeenCalledOnce();
    emitFullscreen(true, 1);
    settle();
    await Promise.resolve();
    await Promise.resolve();
    expect(hasGlass()).toBe(false);
    expect(document.documentElement.classList.contains("macos-fullscreen")).toBe(false);
  });

  it("removes a listener that finishes registration after destruction", async () => {
    let finish!: (stop: () => void) => void;
    nativeWindow.listen.mockReturnValueOnce(new Promise((resolve) => { finish = resolve; }));
    appearance.activateWindowAppearance();
    appearance.disposeWindowAppearance();
    finish(nativeWindow.stop);
    await vi.waitFor(() => expect(nativeWindow.stop).toHaveBeenCalledOnce());
    expect(invoke).not.toHaveBeenCalled();
  });

  it("does not install fullscreen handling on other platforms", async () => {
    platform.isMac = false;
    appearance.activateWindowAppearance();
    await vi.waitFor(() => expect(hasGlass()).toBe(true));
    expect(nativeWindow.listen).not.toHaveBeenCalled();
  });
});
