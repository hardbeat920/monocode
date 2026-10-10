// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { UpdateRow } from "./SettingsView";

(
  globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean }
).IS_REACT_ACT_ENVIRONMENT = true;

const updaterMocks = vi.hoisted(() => ({
  readAppVersion: vi.fn(),
  packageManagedInstall: vi.fn(),
  getPendingRestartVersion: vi.fn(),
  restartToApplyUpdate: vi.fn(),
  installPendingUpdate: vi.fn(),
  runUpdateFlow: vi.fn(),
  listeners: [] as Array<() => void>,
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async () => undefined),
  convertFileSrc: (path: string) => path,
}));
vi.mock("@tauri-apps/api/app", () => ({
  getVersion: vi.fn(async () => "0.1.37"),
  getBundleType: vi.fn(async () => "appimage"),
  BundleType: {
    Nsis: "nsis",
    Msi: "msi",
    Deb: "deb",
    Rpm: "rpm",
    AppImage: "appimage",
    App: "app",
  },
}));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    isMaximized: async () => false,
    onResized: async () => () => {},
  }),
}));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({
  ask: vi.fn(async () => true),
  message: vi.fn(async () => undefined),
}));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: vi.fn() }));
vi.mock("@tauri-apps/plugin-updater", () => ({ check: vi.fn() }));
vi.mock("../../../app/model/updater", () => ({
  getPendingRestartVersion: (...args: unknown[]) =>
    (updaterMocks.getPendingRestartVersion as (...a: unknown[]) => unknown)(
      ...args,
    ),
  readAppVersion: (...args: unknown[]) =>
    (updaterMocks.readAppVersion as (...a: unknown[]) => unknown)(...args),
  packageManagedInstall: (...args: unknown[]) =>
    (updaterMocks.packageManagedInstall as (...a: unknown[]) => unknown)(
      ...args,
    ),
  packageManagerHint: () => "",
  restartToApplyUpdate: (...args: unknown[]) =>
    (updaterMocks.restartToApplyUpdate as (...a: unknown[]) => unknown)(
      ...args,
    ),
  installPendingUpdate: (...args: unknown[]) =>
    (updaterMocks.installPendingUpdate as (...a: unknown[]) => unknown)(
      ...args,
    ),
  runUpdateFlow: (...args: unknown[]) =>
    (updaterMocks.runUpdateFlow as (...a: unknown[]) => unknown)(...args),
  subscribePendingRestart: (listener: () => void) => {
    updaterMocks.listeners.push(listener);
    return () => {
      const at = updaterMocks.listeners.indexOf(listener);
      if (at >= 0) updaterMocks.listeners.splice(at, 1);
    };
  },
}));

let container: HTMLDivElement;
let root: Root | null = null;

function actionButton(): HTMLButtonElement | undefined {
  return [...container.querySelectorAll("button")].find((b) =>
    /Check for updates|Download|Restart now/.test(b.textContent ?? ""),
  ) as HTMLButtonElement | undefined;
}

async function renderRow(): Promise<void> {
  await act(async () => {
    root?.render(createElement(UpdateRow, { onOpenWhatsNew: vi.fn() }));
  });
}

function firePendingRestart(): Promise<void> {
  return act(async () => {
    for (const listener of [...updaterMocks.listeners]) listener();
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

beforeEach(() => {
  updaterMocks.listeners.length = 0;
  vi.clearAllMocks();
  updaterMocks.readAppVersion.mockResolvedValue("0.1.37");
  updaterMocks.packageManagedInstall.mockResolvedValue(null);
  updaterMocks.getPendingRestartVersion.mockReturnValue(null);
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root?.unmount());
  root = null;
  container.remove();
});

describe("UpdateRow restart", () => {
  it("offers Restart now when a restart is already staged", async () => {
    updaterMocks.getPendingRestartVersion.mockReturnValue("0.1.38");
    await renderRow();

    expect(container.textContent).toContain("Restart now");
    expect(container.textContent).toContain(
      "0.1.38 is installed. Restart to apply it.",
    );
  });

  it("routes Restart now through restartToApplyUpdate", async () => {
    updaterMocks.getPendingRestartVersion.mockReturnValue("0.1.38");
    updaterMocks.restartToApplyUpdate.mockImplementation(
      async (onProgress?: (s: unknown) => void) => {
        const next = {
          phase: "restart-required",
          currentVersion: "0.1.37",
          availableVersion: "0.1.38",
        };
        onProgress?.(next);
        return next;
      },
    );
    await renderRow();

    await act(async () => {
      actionButton()?.click();
    });

    expect(updaterMocks.restartToApplyUpdate).toHaveBeenCalledOnce();
    expect(updaterMocks.installPendingUpdate).not.toHaveBeenCalled();
    expect(updaterMocks.runUpdateFlow).not.toHaveBeenCalled();
    expect(container.textContent).toContain("Restart now");
  });

  it("flips to Restart now when an install finishes elsewhere", async () => {
    await renderRow();
    expect(actionButton()?.textContent).toContain("Check for updates");

    updaterMocks.getPendingRestartVersion.mockReturnValue("0.1.38");
    await firePendingRestart();

    expect(container.textContent).toContain("Restart now");
    expect(container.textContent).toContain(
      "0.1.38 is installed. Restart to apply it.",
    );
  });
});
