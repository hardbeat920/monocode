// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from "vitest";
import { syncNativeGlass } from "./appearance";

const platform = vi.hoisted(() => ({ isLinux: false, isMac: false }));
vi.mock("../../../platform/tauri/platform", async (importOriginal) => ({
  ...(await importOriginal<
    typeof import("../../../platform/tauri/platform")
  >()),
  get IS_LINUX() {
    return platform.isLinux;
  },
  get IS_MAC() {
    return platform.isMac;
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

describe("macOS native tint", () => {
  beforeEach(() => {
    vi.resetModules();
    platform.isMac = true;
    document.documentElement.classList.add("is-mac");
    invoke.mockResolvedValue(true);
  });

  const hasNativeTint = () =>
    document.documentElement.classList.contains("has-native-glass-tint");

  it("hands the current tint and opacity to AppKit before clearing the CSS tint", async () => {
    const { applySidebarOpacity, applyThemeTint, syncNativeGlass } =
      await import("./appearance");
    applyThemeTint(0, 100);
    applySidebarOpacity(0.6);
    let settle = (_value: boolean) => {};
    invoke.mockReturnValue(
      new Promise<boolean>((resolve) => {
        settle = resolve;
      }),
    );

    syncNativeGlass("dark");
    expect(invoke).toHaveBeenCalledWith("set_window_glass_enabled", {
      enabled: true,
      background: { r: 46, g: 0, b: 0 },
      opacity: 0.76,
    });
    expect(hasNativeTint()).toBe(false);
    settle(true);
    await vi.waitFor(() => expect(hasNativeTint()).toBe(true));
    expect(hasGlass()).toBe(true);
  });

  it.each([undefined, false])(
    "keeps CSS tint when the host does not acknowledge native tint (%s)",
    async (result) => {
      const { syncNativeGlass } = await import("./appearance");
      invoke.mockResolvedValue(result);
      syncNativeGlass("dark");
      await vi.waitFor(() => expect(hasGlass()).toBe(true));
      expect(hasNativeTint()).toBe(false);
    },
  );

  it.each([
    { base: "0.2", opacity: 0.68 },
    { base: "0", opacity: 0.6 },
  ])(
    "composes the shell's $base tint into the native opacity",
    async ({ base, opacity }) => {
      const { applySidebarOpacity, syncNativeGlass } =
        await import("./appearance");
      document.documentElement.style.setProperty(
        "--window-background-opacity",
        base,
      );
      applySidebarOpacity(0.6);
      syncNativeGlass("dark");
      expect(invoke).toHaveBeenCalledWith("set_window_glass_enabled", {
        enabled: true,
        background: { r: 23, g: 23, b: 23 },
        opacity: expect.closeTo(opacity, 6),
      });
      await vi.waitFor(() => expect(hasNativeTint()).toBe(true));
    },
  );

  it("keeps CSS tint when the native call fails", async () => {
    const { syncNativeGlass } = await import("./appearance");
    invoke.mockRejectedValue(new Error("no native host"));
    syncNativeGlass("dark");
    await vi.waitFor(() => expect(hasGlass()).toBe(true));
    expect(hasNativeTint()).toBe(false);
  });

  it("updates the native tint when appearance settings change after launch", async () => {
    const {
      activateWindowAppearance,
      applySidebarOpacity,
      applyThemeTint,
      applyThemeDarkLightness,
    } = await import("./appearance");
    document.documentElement.style.setProperty(
      "--background-lightness",
      "var(--theme-dark-lightness)",
    );
    applyThemeTint(0, 100);
    applyThemeDarkLightness(10);
    applySidebarOpacity(0.6);
    expect(invoke).not.toHaveBeenCalled();

    activateWindowAppearance();
    await vi.waitFor(() => expect(hasNativeTint()).toBe(true));
    invoke.mockClear();
    applySidebarOpacity(0.4);
    expect(invoke).toHaveBeenLastCalledWith("set_window_glass_enabled", {
      enabled: true,
      background: { r: 51, g: 0, b: 0 },
      opacity: 0.64,
    });
    applyThemeTint(120, 100);
    expect(invoke).toHaveBeenLastCalledWith("set_window_glass_enabled", {
      enabled: true,
      background: { r: 0, g: 51, b: 0 },
      opacity: 0.64,
    });
    applyThemeDarkLightness(20);
    expect(invoke).toHaveBeenLastCalledWith("set_window_glass_enabled", {
      enabled: true,
      background: { r: 0, g: 102, b: 0 },
      opacity: 0.64,
    });
    await vi.waitFor(() => expect(hasNativeTint()).toBe(true));
  });

  it("retains an existing native tint if a later update fails", async () => {
    const { syncNativeGlass } = await import("./appearance");
    syncNativeGlass("dark");
    await vi.waitFor(() => expect(hasNativeTint()).toBe(true));
    invoke.mockRejectedValue(new Error("update failed"));
    syncNativeGlass("dark");
    await Promise.resolve();
    await Promise.resolve();
    expect(hasNativeTint()).toBe(true);
  });

  it("restores opaque CSS before disabling native glass", async () => {
    const { syncNativeGlass } = await import("./appearance");
    syncNativeGlass("dark");
    await vi.waitFor(() => expect(hasNativeTint()).toBe(true));
    invoke.mockClear();
    syncNativeGlass("light");
    expect(hasNativeTint()).toBe(false);
    expect(hasGlass()).toBe(false);
    expect(invoke).not.toHaveBeenCalled();
    await vi.waitFor(() =>
      expect(invoke).toHaveBeenCalledWith(
        "set_window_glass_enabled",
        expect.objectContaining({ enabled: false }),
      ),
    );
  });

  it("ignores a native-tint acknowledgement overtaken by light mode", async () => {
    const { syncNativeGlass } = await import("./appearance");
    let settle = (_value: boolean) => {};
    invoke.mockReturnValue(
      new Promise<boolean>((resolve) => {
        settle = resolve;
      }),
    );
    syncNativeGlass("dark");
    syncNativeGlass("light");
    settle(true);
    await new Promise((resolve) => setTimeout(resolve, 250));
    expect(hasNativeTint()).toBe(false);
    expect(hasGlass()).toBe(false);
  });
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
