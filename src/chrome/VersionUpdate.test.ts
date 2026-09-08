import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { UpdaterSnapshot } from "../lib/updater";
import { VersionUpdate, VersionUpdateButton } from "./VersionUpdate";
import { SidebarUpdateFooter } from "./SidebarUpdate";

const mocks = vi.hoisted(() => ({
  snapshot: { phase: "current", currentVersion: "0.1.40" } as UpdaterSnapshot,
  setSnapshot: vi.fn(),
  inFlight: { current: false },
  effects: [] as (() => (() => void) | void)[],
  readAppVersion: vi.fn(),
  probeForUpdate: vi.fn(),
  runUpdateFlow: vi.fn(),
}));

vi.mock("react", async (importOriginal) => ({
  ...(await importOriginal<typeof import("react")>()),
  useState: () => [mocks.snapshot, mocks.setSnapshot],
  useRef: () => mocks.inFlight,
  useCallback: (callback: unknown) => callback,
  useEffect: (effect: () => (() => void) | void) => mocks.effects.push(effect),
}));
vi.mock("../lib/updater", () => ({
  readAppVersion: mocks.readAppVersion,
  probeForUpdate: mocks.probeForUpdate,
  runUpdateFlow: mocks.runUpdateFlow,
}));

beforeEach(() => {
  vi.clearAllMocks();
  mocks.effects = [];
  mocks.inFlight.current = false;
  mocks.snapshot = { phase: "current", currentVersion: "0.1.40" };
  mocks.readAppVersion.mockResolvedValue("0.1.40");
  mocks.probeForUpdate.mockResolvedValue(null);
  mocks.runUpdateFlow.mockResolvedValue(mocks.snapshot);
});

describe("version update control", () => {
  it.each(["idle", "current", "available", "error"] as const)(
    "shows the installed version as the clickable label (%s)",
    (phase) => {
      const markup = renderToStaticMarkup(
        createElement(VersionUpdateButton, {
          snapshot: {
            phase,
            currentVersion: "0.1.40",
            availableVersion: "0.1.41",
          },
          onClick: vi.fn(),
        }),
      );
      expect(markup).toContain(">v0.1.40</button>");
      expect(markup).toContain("Check for updates");
      expect(markup).not.toContain('disabled=""');
    },
  );

  it.each(["checking", "downloading"] as const)(
    "keeps the version visible and disables clicks while %s",
    (phase) => {
      const markup = renderToStaticMarkup(
        createElement(VersionUpdateButton, {
          snapshot: { phase, currentVersion: "0.1.40", progress: 25 },
          onClick: vi.fn(),
        }),
      );
      expect(markup).toContain(">v0.1.40</button>");
      expect(markup).toContain('disabled=""');
      expect(markup).toContain('aria-busy="true"');
      mocks.snapshot.phase = phase;
      VersionUpdate().props.onClick();
      expect(mocks.runUpdateFlow).not.toHaveBeenCalled();
    },
  );

  it("manually checks and prompts even when an update was previously detected", async () => {
    mocks.snapshot.phase = "available";
    await VersionUpdate().props.onClick();
    expect(mocks.runUpdateFlow).toHaveBeenCalledExactlyOnceWith(
      true,
      mocks.setSnapshot,
    );
  });

  it("prevents duplicate clicks before the updater reports progress", async () => {
    let finish!: () => void;
    mocks.runUpdateFlow.mockReturnValue(
      new Promise<void>((resolve) => {
        finish = resolve;
      }),
    );
    const button = VersionUpdate();
    const first = button.props.onClick();
    await button.props.onClick();
    expect(mocks.runUpdateFlow).toHaveBeenCalledOnce();
    finish();
    await first;
    expect(mocks.inFlight.current).toBe(false);
  });

  it("reads the app version and preserves the automatic update probe", async () => {
    VersionUpdate();
    const cleanup = mocks.effects[0]();
    await vi.waitFor(() => {
      expect(mocks.setSnapshot).toHaveBeenLastCalledWith({
        phase: "current",
        currentVersion: "0.1.40",
      });
    });
    expect(mocks.probeForUpdate).toHaveBeenCalledOnce();
    cleanup?.();
  });

  it("removes the persistent sidebar control while keeping the release notice", () => {
    expect(renderToStaticMarkup(createElement(SidebarUpdateFooter))).toBe("");
    const markup = renderToStaticMarkup(
      createElement(SidebarUpdateFooter, {
        update: { version: "0.1.40" },
        onOpenWhatsNew: vi.fn(),
        onDismissUpdate: vi.fn(),
      }),
    );
    expect(markup).toContain("Updated to 0.1.40");
    expect(markup).not.toContain("Check for updates");
  });
});
