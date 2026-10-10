import { createElement, type ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import type { UpdaterPhase, UpdaterSnapshot } from "../model/updater";
import {
  SidebarUpdate,
  SidebarUpdateFooter,
  isSidebarUpdateActionable,
} from "./SidebarUpdate";

const updaterMocks = vi.hoisted(() => ({
  installPendingUpdate: vi.fn(),
}));

// The updater module reaches for Tauri plugins at import time; stub them so the
// component under test can be imported in the plain node environment.
vi.mock("@tauri-apps/api/app", () => ({
  getVersion: vi.fn(),
  getBundleType: vi.fn().mockResolvedValue("appimage"),
  BundleType: { Nsis: "nsis", Msi: "msi", Deb: "deb", Rpm: "rpm", AppImage: "appimage", App: "app" },
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({
  ask: vi.fn(),
  message: vi.fn(),
}));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: vi.fn() }));
vi.mock("@tauri-apps/plugin-updater", () => ({ check: vi.fn() }));
vi.mock("../model/updater", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../model/updater")>();
  return {
    ...actual,
    installPendingUpdate: (
      ...args: Parameters<typeof actual.installPendingUpdate>
    ) => updaterMocks.installPendingUpdate(...args),
  };
});

function installButtonClick() {
  let onClick: (() => void) | undefined;
  // SidebarUpdate renders the install button nested in a wrapper div (with an
  // optional dismiss button for restart-required), so walk the element tree.
  function findInstallClick(node: unknown): (() => void) | undefined {
    if (!node || typeof node !== "object") return undefined;
    const el = node as ReactElement<{ onClick?: () => void; children?: unknown } & Record<string, unknown>>;
    if (
      el.type === "button" &&
      typeof el.props?.onClick === "function" &&
      el.props?.["aria-label"] !== "Dismiss restart notification"
    ) {
      return el.props.onClick;
    }
    const children = el.props?.children;
    if (Array.isArray(children)) {
      for (const child of children) {
        const found = findInstallClick(child);
        if (found) return found;
      }
      return undefined;
    }
    return findInstallClick(children);
  }
  function Capture() {
    const tree = SidebarUpdate({
      snapshot: {
        phase: "available",
        currentVersion: "0.1.37",
        availableVersion: "0.1.38",
      },
      onSnapshot: vi.fn(),
    }) as ReactElement;
    onClick = findInstallClick(tree);
    return tree;
  }
  renderToStaticMarkup(createElement(Capture));
  if (!onClick) throw new Error("expected the install button handler");
  return onClick;
}

describe("isSidebarUpdateActionable", () => {
  it("only claims sidebar space for an update the user can act on", () => {
    const phases: UpdaterPhase[] = [
      "idle",
      "checking",
      "current",
      "available",
      "downloading",
      "restart-required",
      "error",
    ];
    const actionable = phases.filter((phase) =>
      isSidebarUpdateActionable({ phase, currentVersion: "0.1.37" }),
    );
    expect(actionable).toEqual(["available", "downloading", "restart-required"]);
  });
});

describe("SidebarUpdate", () => {
  it("offers the install action for an available version", () => {
    const markup = renderToStaticMarkup(
      createElement(SidebarUpdate, {
        snapshot: {
          phase: "available",
          currentVersion: "0.1.37",
          availableVersion: "0.1.38",
        },
        onSnapshot: vi.fn(),
      }),
    );

    expect(markup).toContain("Update to 0.1.38");
    expect(markup).toContain("v0.1.37");
    expect(markup).not.toContain('disabled=""');
  });

  it("reports download progress and blocks a second click", () => {
    const markup = renderToStaticMarkup(
      createElement(SidebarUpdate, {
        snapshot: {
          phase: "downloading",
          currentVersion: "0.1.37",
          availableVersion: "0.1.38",
          progress: 42,
        },
        onSnapshot: vi.fn(),
      }),
    );

    expect(markup).toContain("Downloading 42%");
    expect(markup).toContain('disabled=""');
  });

  it("offers a restart action once the update is installed", () => {
    const markup = renderToStaticMarkup(
      createElement(SidebarUpdate, {
        snapshot: {
          phase: "restart-required",
          currentVersion: "0.1.37",
          availableVersion: "0.1.38",
        },
        onSnapshot: vi.fn(),
      }),
    );

    expect(markup).toContain("Restart to update to 0.1.38");
    expect(markup).not.toContain('disabled=""');
  });

  it("lets a deferred restart be dismissed without losing the install", () => {
    const withoutDismiss = renderToStaticMarkup(
      createElement(SidebarUpdate, {
        snapshot: {
          phase: "restart-required",
          currentVersion: "0.1.37",
          availableVersion: "0.1.38",
        },
        onSnapshot: vi.fn(),
      }),
    );
    const withDismiss = renderToStaticMarkup(
      createElement(SidebarUpdate, {
        snapshot: {
          phase: "restart-required",
          currentVersion: "0.1.37",
          availableVersion: "0.1.38",
        },
        onSnapshot: vi.fn(),
        onDismiss: vi.fn(),
      }),
    );

    expect(withoutDismiss).not.toContain("Dismiss restart notification");
    expect(withDismiss).toContain('aria-label="Dismiss restart notification"');
  });

  it("ignores a second click while readAppVersion is still pending", async () => {
    // installPendingUpdate awaits readAppVersion before it reports "downloading",
    // so a second click can still land while `busy` is false. The hanging mock
    // is that window.
    let releaseVersionRead!: (snapshot: UpdaterSnapshot) => void;
    updaterMocks.installPendingUpdate.mockReset();
    updaterMocks.installPendingUpdate.mockImplementation(
      () =>
        new Promise<UpdaterSnapshot>((resolve) => {
          releaseVersionRead = resolve;
        }),
    );

    const onClick = installButtonClick();
    const first = Promise.resolve(onClick());
    const second = Promise.resolve(onClick());

    expect(updaterMocks.installPendingUpdate).toHaveBeenCalledOnce();

    releaseVersionRead({
      phase: "downloading",
      currentVersion: "0.1.37",
      availableVersion: "0.1.38",
    });
    await Promise.all([first, second]);
    expect(updaterMocks.installPendingUpdate).toHaveBeenCalledOnce();
  });
});

describe("SidebarUpdateFooter", () => {
  // renderToStaticMarkup never runs effects, so the automatic probe stays in its
  // initial `idle` phase here — exactly the state that used to render a
  // permanent "Check for updates" row.
  it("stays silent while the automatic probe has nothing to offer", () => {
    expect(renderToStaticMarkup(createElement(SidebarUpdateFooter, {}))).toBe(
      "",
    );
  });

  it("still shows the post-install card without an update row", () => {
    const markup = renderToStaticMarkup(
      createElement(SidebarUpdateFooter, {
        update: { version: "0.1.37" },
        onOpenWhatsNew: vi.fn(),
        onDismissUpdate: vi.fn(),
      }),
    );

    expect(markup).toContain("Updated to 0.1.37");
    expect(markup).toContain("What&#x27;s new");
    expect(markup).not.toContain("Check for updates");
    expect(markup).not.toContain("Update to");
  });
});
