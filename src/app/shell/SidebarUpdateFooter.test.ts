// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SidebarUpdateFooter } from "./SidebarUpdate";

(
  globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean }
).IS_REACT_ACT_ENVIRONMENT = true;

const updaterMocks = vi.hoisted(() => ({
  readAppVersion: vi.fn(),
  getPendingRestartVersion: vi.fn(),
  probeForUpdate: vi.fn(),
  installPendingUpdate: vi.fn(),
  restartToApplyUpdate: vi.fn(),
  listeners: [] as Array<() => void>,
}));

vi.mock("@tauri-apps/api/app", () => ({
  getVersion: vi.fn(),
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
  ask: vi.fn(),
  message: vi.fn(),
}));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: vi.fn() }));
vi.mock("@tauri-apps/plugin-updater", () => ({ check: vi.fn() }));
vi.mock("../model/updater", () => ({
  getPendingRestartVersion: (...args: unknown[]) =>
    (updaterMocks.getPendingRestartVersion as (...a: unknown[]) => unknown)(
      ...args,
    ),
  readAppVersion: (...args: unknown[]) =>
    (updaterMocks.readAppVersion as (...a: unknown[]) => unknown)(...args),
  probeForUpdate: (...args: unknown[]) =>
    (updaterMocks.probeForUpdate as (...a: unknown[]) => unknown)(...args),
  installPendingUpdate: (...args: unknown[]) =>
    (updaterMocks.installPendingUpdate as (...a: unknown[]) => unknown)(
      ...args,
    ),
  restartToApplyUpdate: (...args: unknown[]) =>
    (updaterMocks.restartToApplyUpdate as (...a: unknown[]) => unknown)(
      ...args,
    ),
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

function dismissButton(): HTMLButtonElement | null {
  return container.querySelector(
    'button[aria-label="Dismiss restart notification"]',
  );
}

async function renderFooter(): Promise<void> {
  await act(async () => {
    root?.render(createElement(SidebarUpdateFooter, {}));
  });
}

function firePendingRestart(): Promise<void> {
  return act(async () => {
    for (const listener of [...updaterMocks.listeners]) listener();
    // The subscription handler awaits readAppVersion before setSnapshot.
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

beforeEach(() => {
  updaterMocks.listeners.length = 0;
  vi.clearAllMocks();
  updaterMocks.readAppVersion.mockResolvedValue("0.1.37");
  updaterMocks.getPendingRestartVersion.mockReturnValue(null);
  updaterMocks.probeForUpdate.mockResolvedValue(null);
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root?.unmount());
  root = null;
  container.remove();
});

describe("SidebarUpdateFooter deferred restart", () => {
  it("hides the row after dismissing a deferred restart", async () => {
    updaterMocks.getPendingRestartVersion.mockReturnValue("0.1.38");
    await renderFooter();

    expect(container.textContent).toContain("Restart to update to 0.1.38");
    expect(dismissButton()).not.toBeNull();

    await act(async () => {
      dismissButton()?.click();
    });

    expect(container.textContent).not.toContain("Restart to update");
    expect(container.innerHTML).toBe("");
  });

  it("shows a newer deferred restart after a dismiss", async () => {
    updaterMocks.getPendingRestartVersion.mockReturnValue("0.1.38");
    await renderFooter();
    await act(async () => {
      dismissButton()?.click();
    });
    expect(container.innerHTML).toBe("");

    updaterMocks.getPendingRestartVersion.mockReturnValue("0.1.39");
    await firePendingRestart();

    expect(container.textContent).toContain("Restart to update to 0.1.39");
  });

  it("syncs to restart-required when an install finishes elsewhere", async () => {
    updaterMocks.probeForUpdate.mockResolvedValue({ version: "0.1.38" });
    await renderFooter();
    expect(container.textContent).toContain("Update to 0.1.38");

    updaterMocks.getPendingRestartVersion.mockReturnValue("0.1.38");
    await firePendingRestart();

    expect(container.textContent).toContain("Restart to update to 0.1.38");
    expect(container.textContent).not.toContain("Update to 0.1.38");
  });
});
