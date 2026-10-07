import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { getVersion, getBundleType, check, message, ask, relaunch } = vi.hoisted(() => ({
  getVersion: vi.fn(),
  getBundleType: vi.fn(),
  check: vi.fn(),
  message: vi.fn(),
  ask: vi.fn(),
  relaunch: vi.fn(),
}));

vi.mock("@tauri-apps/api/app", () => ({
  getVersion,
  getBundleType,
  BundleType: { Nsis: "nsis", Msi: "msi", Deb: "deb", Rpm: "rpm", AppImage: "appimage", App: "app" },
}));
vi.mock("@tauri-apps/plugin-updater", () => ({ check }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ ask, message }));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch }));
vi.mock("../../features/settings/model/sounds", () => ({ announceUpdateAvailable: vi.fn() }));

import { probeForUpdate, runUpdateFlow } from "./updater";

describe("updater", () => {
  beforeEach(() => {
    getBundleType.mockResolvedValue("appimage");
  });

  afterEach(() => {
    vi.resetAllMocks();
  });

  it("keeps automatic checks quiet when updater endpoints are missing", async () => {
    getVersion.mockResolvedValue("0.1.23");
    check.mockRejectedValue(new Error("Updater does not have any endpoints set"));

    await expect(runUpdateFlow(false)).resolves.toEqual({
      phase: "idle",
      currentVersion: "0.1.23",
    });
    expect(message).not.toHaveBeenCalled();
  });

  it("points manual checks without updater endpoints to GitHub releases", async () => {
    getVersion.mockResolvedValue("0.1.23");
    check.mockRejectedValue(new Error("Updater does not have any endpoints set"));

    await expect(runUpdateFlow(true)).resolves.toEqual({
      phase: "idle",
      currentVersion: "0.1.23",
    });
    expect(message).toHaveBeenCalledWith(
      expect.stringContaining("https://github.com/hardbeat920/monocode/releases/latest"),
      { title: "MonoCode" },
    );
  });

  it("still reports real updater failures", async () => {
    getVersion.mockResolvedValue("0.1.23");
    check.mockRejectedValue(new Error("network failed"));

    await expect(runUpdateFlow(true)).resolves.toMatchObject({
      phase: "error",
      error: "network failed",
    });
    expect(message).toHaveBeenCalledOnce();
  });

  it.each([
    ["deb", "sudo apt install"],
    ["rpm", "sudo dnf install"],
  ])("sends %s installs to their package manager without checking the feed", async (kind, hint) => {
    getVersion.mockResolvedValue("0.9.0");
    getBundleType.mockResolvedValue(kind);

    await expect(runUpdateFlow(true)).resolves.toEqual({
      phase: "idle",
      currentVersion: "0.9.0",
      packageManaged: kind,
    });
    expect(check).not.toHaveBeenCalled();
    expect(message).toHaveBeenCalledWith(expect.stringContaining(hint), {
      title: "MonoCode",
    });
  });

  it("keeps the automatic probe silent on package-managed installs", async () => {
    getBundleType.mockResolvedValue("deb");

    await expect(probeForUpdate()).resolves.toBeNull();
    expect(check).not.toHaveBeenCalled();
  });

  it("still checks the feed for AppImage installs", async () => {
    getVersion.mockResolvedValue("0.9.0");
    check.mockResolvedValue(null);

    await expect(runUpdateFlow(false)).resolves.toEqual({
      phase: "current",
      currentVersion: "0.9.0",
    });
    expect(check).toHaveBeenCalledOnce();
  });

  it("treats a feed without this platform as unavailable, not as a failure", async () => {
    getVersion.mockResolvedValue("0.9.0");
    check.mockRejectedValue(
      new Error(
        'None of the fallback platforms `["linux-x86_64-deb", "linux-x86_64"]` were found in the response `platforms` object',
      ),
    );

    await expect(runUpdateFlow(true)).resolves.toEqual({
      phase: "idle",
      currentVersion: "0.9.0",
    });
    expect(message).toHaveBeenCalledWith(
      expect.stringContaining("aren't available for this install yet"),
      { title: "MonoCode" },
    );
  });
});
