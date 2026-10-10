import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  announce: vi.fn(),
  check: vi.fn(),
  downloadAndInstall: vi.fn(),
  getVersion: vi.fn(),
  ask: vi.fn(),
  message: vi.fn(),
  relaunch: vi.fn(),
  remember: vi.fn(),
}));

vi.mock("@tauri-apps/api/app", () => ({
  getVersion: mocks.getVersion,
  getBundleType: vi.fn().mockResolvedValue("appimage"),
  BundleType: {
    Nsis: "nsis",
    Msi: "msi",
    Deb: "deb",
    Rpm: "rpm",
    AppImage: "appimage",
    App: "app",
  },
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({
  ask: mocks.ask,
  message: mocks.message,
}));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: mocks.relaunch }));
vi.mock("@tauri-apps/plugin-updater", () => ({ check: mocks.check }));
vi.mock("../../features/settings/model/sounds", () => ({
  announceUpdateAvailable: mocks.announce,
}));
vi.mock("./updateNotice", () => ({ rememberInstalledUpdate: mocks.remember }));

beforeEach(() => {
  vi.clearAllMocks();
  vi.resetModules();
  mocks.getVersion.mockResolvedValue("0.1.22");
  mocks.relaunch.mockResolvedValue(undefined);
  mocks.message.mockResolvedValue(undefined);
  mocks.ask.mockResolvedValue(true);
});

async function updaterWithPendingUpdate() {
  const update = {
    version: "0.1.23",
    downloadAndInstall: mocks.downloadAndInstall,
  };
  mocks.check.mockResolvedValue(update);
  const updater = await import("./updater");
  await updater.probeForUpdate();
  return updater;
}

describe("installPendingUpdate", () => {
  it("records a successful installation before relaunching", async () => {
    mocks.downloadAndInstall.mockResolvedValue(undefined);
    mocks.ask.mockResolvedValue(true);
    const updater = await updaterWithPendingUpdate();

    await updater.installPendingUpdate();

    expect(mocks.remember).toHaveBeenCalledWith("0.1.23");
    expect(mocks.ask).toHaveBeenCalledWith(
      expect.stringContaining("Restart now"),
      expect.objectContaining({
        title: "Update ready",
        okLabel: "Restart now",
        cancelLabel: "Later",
      }),
    );
    expect(mocks.relaunch).toHaveBeenCalledOnce();
    expect(mocks.remember.mock.invocationCallOrder[0]).toBeLessThan(
      mocks.relaunch.mock.invocationCallOrder[0]!,
    );
  });

  it("holds the app open when the user defers the restart", async () => {
    mocks.downloadAndInstall.mockResolvedValue(undefined);
    mocks.ask.mockResolvedValue(false);
    const updater = await updaterWithPendingUpdate();

    const result = await updater.installPendingUpdate();

    expect(result.phase).toBe("restart-required");
    expect(result.availableVersion).toBe("0.1.23");
    expect(mocks.remember).toHaveBeenCalledWith("0.1.23");
    expect(mocks.relaunch).not.toHaveBeenCalled();
  });

  it("does not record or relaunch after installation fails", async () => {
    mocks.downloadAndInstall.mockRejectedValue(new Error("install failed"));
    const updater = await updaterWithPendingUpdate();

    const result = await updater.installPendingUpdate();

    expect(result.phase).toBe("error");
    expect(mocks.remember).not.toHaveBeenCalled();
    expect(mocks.relaunch).not.toHaveBeenCalled();
  });

  it("does not record when no update is pending", async () => {
    const updater = await import("./updater");

    expect((await updater.installPendingUpdate()).phase).toBe("idle");
    expect(mocks.remember).not.toHaveBeenCalled();
    expect(mocks.relaunch).not.toHaveBeenCalled();
  });

  it("reports a relaunch failure as restart-required, not as an install failure", async () => {
    mocks.downloadAndInstall.mockResolvedValue(undefined);
    mocks.ask.mockResolvedValue(true);
    mocks.relaunch.mockRejectedValue(new Error("relaunch failed"));
    const updater = await updaterWithPendingUpdate();

    const result = await updater.installPendingUpdate();

    expect(result.phase).toBe("restart-required");
    expect(result.error).toContain("relaunch failed");
    expect(mocks.remember).toHaveBeenCalledWith("0.1.23");
    expect(mocks.message).toHaveBeenCalledWith(
      expect.stringContaining("Couldn't restart"),
      { title: "MonoCode" },
    );
    expect(mocks.message).not.toHaveBeenCalledWith(
      expect.stringContaining("Couldn't install"),
      expect.anything(),
    );
    // Module state stays consistent: the update is still staged.
    const again = await updater.installPendingUpdate();
    expect(again.phase).toBe("restart-required");
    expect(again.availableVersion).toBe("0.1.23");
  });
});

describe("restart-required flow", () => {
  async function updaterWithDeferredRestart() {
    mocks.downloadAndInstall.mockResolvedValue(undefined);
    mocks.ask.mockResolvedValue(false);
    const updater = await updaterWithPendingUpdate();
    const installed = await updater.installPendingUpdate();
    expect(installed.phase).toBe("restart-required");
    vi.clearAllMocks();
    mocks.getVersion.mockResolvedValue("0.1.22");
    mocks.relaunch.mockResolvedValue(undefined);
    mocks.message.mockResolvedValue(undefined);
    return updater;
  }

  it("keeps the probe quiet while a restart is pending", async () => {
    const updater = await updaterWithDeferredRestart();

    await expect(updater.probeForUpdate()).resolves.toBeNull();
    expect(mocks.check).not.toHaveBeenCalled();
  });

  it("stays silent on automatic checks while a restart is pending", async () => {
    const updater = await updaterWithDeferredRestart();
    mocks.ask.mockResolvedValue(true);

    const result = await updater.runUpdateFlow(false);

    expect(result).toMatchObject({
      phase: "restart-required",
      availableVersion: "0.1.23",
    });
    expect(mocks.ask).not.toHaveBeenCalled();
    expect(mocks.relaunch).not.toHaveBeenCalled();
    expect(mocks.check).not.toHaveBeenCalled();
  });

  it("re-offers the restart on manual checks and relaunches when accepted", async () => {
    const updater = await updaterWithDeferredRestart();
    mocks.ask.mockResolvedValue(true);

    const result = await updater.runUpdateFlow(true);

    expect(mocks.ask).toHaveBeenCalledWith(
      expect.stringContaining("Restart now"),
      expect.objectContaining({ title: "Update ready" }),
    );
    expect(mocks.relaunch).toHaveBeenCalledOnce();
    // relaunch should exit; if it returns, the restart stays staged so a
    // no-op relaunch never desyncs the snapshot from module state.
    expect(result).toMatchObject({
      phase: "restart-required",
      availableVersion: "0.1.23",
    });
  });

  it("holds the app open when the manual restart offer is declined", async () => {
    const updater = await updaterWithDeferredRestart();
    mocks.ask.mockResolvedValue(false);

    const result = await updater.runUpdateFlow(true);

    expect(result.phase).toBe("restart-required");
    expect(result.availableVersion).toBe("0.1.23");
    expect(mocks.relaunch).not.toHaveBeenCalled();
  });
});

describe("restartToApplyUpdate", () => {
  it("returns idle when no restart is staged", async () => {
    const updater = await import("./updater");

    const result = await updater.restartToApplyUpdate();

    expect(result.phase).toBe("idle");
    expect(mocks.relaunch).not.toHaveBeenCalled();
  });

  it("never rejects when relaunch fails", async () => {
    mocks.downloadAndInstall.mockResolvedValue(undefined);
    mocks.ask.mockResolvedValue(false);
    const updater = await updaterWithPendingUpdate();
    await updater.installPendingUpdate();
    vi.clearAllMocks();
    mocks.getVersion.mockResolvedValue("0.1.22");
    mocks.relaunch.mockRejectedValue(new Error("nope"));
    mocks.message.mockResolvedValue(undefined);

    const result = await updater.restartToApplyUpdate();

    expect(result.phase).toBe("restart-required");
    expect(result.error).toContain("nope");
    expect(mocks.message).toHaveBeenCalledWith(
      expect.stringContaining("Couldn't restart"),
      { title: "MonoCode" },
    );
  });

  it("tolerates a throwing onProgress without rejecting", async () => {
    mocks.downloadAndInstall.mockResolvedValue(undefined);
    mocks.ask.mockResolvedValue(false);
    const updater = await updaterWithPendingUpdate();
    const throwing = () => {
      throw new Error("ui boom");
    };

    await expect(updater.installPendingUpdate(throwing)).resolves.toMatchObject(
      { phase: "restart-required" },
    );
    mocks.relaunch.mockRejectedValue(new Error("nope"));
    await expect(updater.restartToApplyUpdate(throwing)).resolves.toMatchObject(
      { phase: "restart-required" },
    );
    await expect(updater.runUpdateFlow(false, throwing)).resolves.toMatchObject(
      { phase: "restart-required" },
    );
  });

  it("re-offers the restart when a staged install is triggered again", async () => {
    mocks.downloadAndInstall.mockResolvedValue(undefined);
    mocks.ask.mockResolvedValue(false);
    const updater = await updaterWithPendingUpdate();
    const staged = await updater.installPendingUpdate();
    expect(staged.phase).toBe("restart-required");
    vi.clearAllMocks();
    mocks.getVersion.mockResolvedValue("0.1.22");
    mocks.ask.mockResolvedValue(true);
    mocks.relaunch.mockResolvedValue(undefined);
    mocks.message.mockResolvedValue(undefined);

    // pendingUpdate is gone but the restart is staged: one call restarts.
    const result = await updater.installPendingUpdate();

    expect(mocks.ask).toHaveBeenCalledWith(
      expect.stringContaining("Restart now"),
      expect.objectContaining({ title: "Update ready" }),
    );
    expect(mocks.relaunch).toHaveBeenCalledOnce();
    expect(result.phase).toBe("restart-required");
  });

  it("holds the staged restart when the re-offer is declined", async () => {
    mocks.downloadAndInstall.mockResolvedValue(undefined);
    mocks.ask.mockResolvedValue(false);
    const updater = await updaterWithPendingUpdate();
    await updater.installPendingUpdate();
    vi.clearAllMocks();
    mocks.getVersion.mockResolvedValue("0.1.22");
    mocks.ask.mockResolvedValue(false);

    const result = await updater.installPendingUpdate();

    expect(result).toMatchObject({
      phase: "restart-required",
      availableVersion: "0.1.23",
    });
    expect(mocks.relaunch).not.toHaveBeenCalled();
  });

  it("notifies pending-restart subscribers when an install stages", async () => {
    mocks.downloadAndInstall.mockResolvedValue(undefined);
    mocks.ask.mockResolvedValue(false);
    const updater = await updaterWithPendingUpdate();
    const listener = vi.fn();
    const unsubscribe = updater.subscribePendingRestart(listener);

    await updater.installPendingUpdate();

    expect(listener).toHaveBeenCalledOnce();
    expect(updater.getPendingRestartVersion()).toBe("0.1.23");
    unsubscribe();
  });
});
