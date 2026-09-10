import { createElement, type ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import type { UpdaterPhase, UpdaterSnapshot } from "../lib/updater";
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
vi.mock("@tauri-apps/api/app", () => ({ getVersion: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({
  ask: vi.fn(),
  message: vi.fn(),
}));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: vi.fn() }));
vi.mock("@tauri-apps/plugin-updater", () => ({ check: vi.fn() }));
vi.mock("../lib/updater", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/updater")>();
  return {
    ...actual,
    installPendingUpdate: (
      ...args: Parameters<typeof actual.installPendingUpdate>
    ) => updaterMocks.installPendingUpdate(...args),
  };
});

function installButtonClick() {
  let onClick: (() => void) | undefined;
  function Capture() {
    const tree = SidebarUpdate({
      snapshot: {
        phase: "available",
        currentVersion: "0.1.37",
        availableVersion: "0.1.38",
      },
      onSnapshot: vi.fn(),
    }) as ReactElement<{ onClick: () => void }>;
    onClick = tree.props.onClick;
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
      "error",
    ];
    const actionable = phases.filter((phase) =>
      isSidebarUpdateActionable({ phase, currentVersion: "0.1.37" }),
    );
    expect(actionable).toEqual(["available", "downloading"]);
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
