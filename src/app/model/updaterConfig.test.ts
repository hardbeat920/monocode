import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  getVersion: vi.fn(),
  getBundleType: vi.fn(),
  check: vi.fn(),
  message: vi.fn(),
  ask: vi.fn(),
  relaunch: vi.fn(),
}));

vi.mock("@tauri-apps/api/app", () => ({
  getVersion: mocks.getVersion,
  getBundleType: mocks.getBundleType,
  BundleType: {
    Nsis: "nsis",
    Msi: "msi",
    Deb: "deb",
    Rpm: "rpm",
    AppImage: "appimage",
    App: "app",
  },
}));
vi.mock("@tauri-apps/plugin-updater", () => ({ check: mocks.check }));
vi.mock("@tauri-apps/plugin-dialog", () => ({
  ask: mocks.ask,
  message: mocks.message,
}));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: mocks.relaunch }));
vi.mock("../../features/settings/model/sounds", () => ({
  announceUpdateAvailable: vi.fn(),
}));

describe("updater", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.resetModules();
    mocks.getBundleType.mockResolvedValue("appimage");
  });

  afterEach(() => {
    vi.clearAllMocks();
    vi.resetModules();
  });

  it("keeps automatic checks quiet when updater endpoints are missing", async () => {
    mocks.getVersion.mockResolvedValue("0.1.23");
    mocks.check.mockRejectedValue(
      new Error("Updater does not have any endpoints set"),
    );
    const { runUpdateFlow } = await import("./updater");

    await expect(runUpdateFlow(false)).resolves.toEqual({
      phase: "idle",
      currentVersion: "0.1.23",
    });
    expect(mocks.message).not.toHaveBeenCalled();
  });

  it("points manual checks without updater endpoints to GitHub releases", async () => {
    mocks.getVersion.mockResolvedValue("0.1.23");
    mocks.check.mockRejectedValue(
      new Error("Updater does not have any endpoints set"),
    );
    const { runUpdateFlow } = await import("./updater");

    await expect(runUpdateFlow(true)).resolves.toEqual({
      phase: "idle",
      currentVersion: "0.1.23",
    });
    expect(mocks.message).toHaveBeenCalledWith(
      expect.stringContaining(
        "https://github.com/hardbeat920/monocode/releases/latest",
      ),
      { title: "MonoCode" },
    );
  });

  it("still reports real updater failures", async () => {
    mocks.getVersion.mockResolvedValue("0.1.23");
    mocks.check.mockRejectedValue(new Error("network failed"));
    const { runUpdateFlow } = await import("./updater");

    await expect(runUpdateFlow(true)).resolves.toMatchObject({
      phase: "error",
      error: "network failed",
    });
    expect(mocks.message).toHaveBeenCalledOnce();
  });

  it.each(["deb", "rpm"] as const)(
    "names one %s installer and the releases URL",
    async (kind) => {
      const { packageManagerHint } = await import("./updater");
      const hint = packageManagerHint(kind);
      expect(hint).toContain(
        "https://github.com/hardbeat920/monocode/releases/latest",
      );
      expect(hint).not.toMatch(/[*<>]/);
      expect(hint).toContain("Replace the file name");
      expect(hint).toContain(
        kind === "deb"
          ? "sudo apt install ./MonoCode_X.Y.Z_amd64.deb"
          : "sudo dnf install ./MonoCode-X.Y.Z-1.x86_64.rpm",
      );
    },
  );

  it.each([
    ["deb", "sudo apt install"],
    ["rpm", "sudo dnf install"],
  ])(
    "sends %s installs to their package manager without checking the feed",
    async (kind, hint) => {
      mocks.getVersion.mockResolvedValue("0.9.0");
      mocks.getBundleType.mockResolvedValue(kind);
      const { runUpdateFlow } = await import("./updater");

      await expect(runUpdateFlow(true)).resolves.toEqual({
        phase: "idle",
        currentVersion: "0.9.0",
        packageManaged: kind,
      });
      expect(mocks.check).not.toHaveBeenCalled();
      expect(mocks.message).toHaveBeenCalledWith(
        expect.stringContaining(hint),
        {
          title: "MonoCode",
        },
      );
    },
  );

  it("keeps the automatic probe silent on package-managed installs", async () => {
    mocks.getBundleType.mockResolvedValue("deb");
    const { probeForUpdate } = await import("./updater");

    await expect(probeForUpdate()).resolves.toBeNull();
    expect(mocks.check).not.toHaveBeenCalled();
  });

  it("still checks the feed for AppImage installs", async () => {
    mocks.getVersion.mockResolvedValue("0.9.0");
    mocks.check.mockResolvedValue(null);
    const { runUpdateFlow } = await import("./updater");

    await expect(runUpdateFlow(false)).resolves.toEqual({
      phase: "current",
      currentVersion: "0.9.0",
    });
    expect(mocks.check).toHaveBeenCalledOnce();
  });

  it("treats a feed without this platform as unavailable, not as a failure", async () => {
    mocks.getVersion.mockResolvedValue("0.9.0");
    mocks.check.mockRejectedValue(
      new Error(
        'None of the fallback platforms `["linux-x86_64-deb", "linux-x86_64"]` were found in the response `platforms` object',
      ),
    );
    const { runUpdateFlow } = await import("./updater");

    await expect(runUpdateFlow(true)).resolves.toEqual({
      phase: "idle",
      currentVersion: "0.9.0",
    });
    expect(mocks.message).toHaveBeenCalledWith(
      expect.stringContaining("aren't available for this install yet"),
      { title: "MonoCode" },
    );
  });
});
