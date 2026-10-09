import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { claimTrackpadMagnify } from "./trackpadZoom";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
  isTauri: vi.fn(),
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

const releases: (() => void)[] = [];

function claim(handler = vi.fn()) {
  const release = claimTrackpadMagnify(handler);
  releases.push(release);
  return release;
}

function magnify(subscription: number, delta: number) {
  vi.mocked(listen).mock.calls[subscription][1]({
    event: "trackpad_magnify",
    id: subscription,
    payload: delta,
  });
}

// Let subscriptions, cleanup and rejection reporting finish before asserting.
async function flushEvents() {
  await new Promise((resolve) => setTimeout(resolve, 0));
}

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(isTauri).mockReturnValue(true);
  vi.mocked(invoke).mockResolvedValue(undefined);
  vi.mocked(listen).mockResolvedValue(vi.fn());
  vi.spyOn(console, "error").mockImplementation(() => {});
});

afterEach(async () => {
  releases.splice(0).forEach((release) => release());
  await flushEvents();
  vi.restoreAllMocks();
});

describe("claimTrackpadMagnify", () => {
  it("does nothing outside Tauri", async () => {
    vi.mocked(isTauri).mockReturnValue(false);
    const release = claim();
    release();
    release();
    await flushEvents();

    expect(listen).not.toHaveBeenCalled();
    expect(invoke).not.toHaveBeenCalled();
    expect(console.error).not.toHaveBeenCalled();
  });

  it("shares the native hook and routes pinches to the latest remaining claim", async () => {
    const stop = vi.fn();
    vi.mocked(listen).mockResolvedValue(stop);
    const first = vi.fn();
    const second = vi.fn();
    const releaseFirst = claim(first);
    const releaseSecond = claim(second);
    await flushEvents();

    expect(listen).toHaveBeenCalledExactlyOnceWith(
      "trackpad_magnify",
      expect.any(Function),
    );
    expect(invoke).toHaveBeenCalledExactlyOnceWith(
      "set_trackpad_zoom_enabled",
      { enabled: true },
    );
    magnify(0, 0.25);
    expect(second).toHaveBeenCalledExactlyOnceWith(0.25);
    expect(first).not.toHaveBeenCalled();

    releaseSecond();
    releaseSecond();
    magnify(0, -0.1);
    expect(first).toHaveBeenCalledExactlyOnceWith(-0.1);
    expect(stop).not.toHaveBeenCalled();
    expect(invoke).toHaveBeenCalledTimes(1);

    releaseFirst();
    releaseFirst();
    magnify(0, 0.5);
    await flushEvents();
    expect(first).toHaveBeenCalledTimes(1);
    expect(stop).toHaveBeenCalledOnce();
    expect(invoke).toHaveBeenCalledTimes(2);
    expect(invoke).toHaveBeenLastCalledWith("set_trackpad_zoom_enabled", {
      enabled: false,
    });
  });

  it("cleans up a late subscription without routing its events to a new claim", async () => {
    let resolveOld!: (stop: UnlistenFn) => void;
    vi.mocked(listen).mockReturnValueOnce(
      new Promise((resolve) => {
        resolveOld = resolve;
      }),
    );
    const oldHandler = vi.fn();
    const releaseOld = claim(oldHandler);
    releaseOld();
    const stopNew = vi.fn();
    vi.mocked(listen).mockResolvedValueOnce(stopNew);
    const newHandler = vi.fn();
    const releaseNew = claim(newHandler);
    await flushEvents();

    magnify(0, 0.2);
    magnify(1, 0.3);
    expect(oldHandler).not.toHaveBeenCalled();
    expect(newHandler).toHaveBeenCalledExactlyOnceWith(0.3);

    const stopOld = vi.fn();
    resolveOld(stopOld);
    await flushEvents();
    expect(stopOld).toHaveBeenCalledOnce();
    expect(stopNew).not.toHaveBeenCalled();
    magnify(0, 0.4);
    magnify(1, 0.5);
    expect(newHandler.mock.calls).toEqual([[0.3], [0.5]]);

    releaseNew();
    await flushEvents();
    expect(stopNew).toHaveBeenCalledOnce();
    expect(vi.mocked(invoke).mock.calls.map(([, args]) => args)).toEqual([
      { enabled: true },
      { enabled: false },
      { enabled: true },
      { enabled: false },
    ]);
  });

  it.each([false, true])(
    "handles subscription rejection (released before rejection: %s) and allows a new subscription",
    async (releaseBeforeRejection) => {
      let reject!: (error: Error) => void;
      vi.mocked(listen).mockReturnValueOnce(
        new Promise((_, fail) => {
          reject = fail;
        }),
      );
      const release = claim();
      if (releaseBeforeRejection) release();
      const error = new Error("Subscription failed");
      reject(error);
      await flushEvents();
      if (!releaseBeforeRejection) release();
      await flushEvents();
      expect(console.error).toHaveBeenCalledExactlyOnceWith(error);

      const handler = vi.fn();
      claim(handler);
      await flushEvents();
      magnify(1, 0.1);
      expect(handler).toHaveBeenCalledExactlyOnceWith(0.1);
    },
  );

  it.each(["throw", "reject"])(
    "handles cleanup failures that %s",
    async (kind) => {
      const error = new Error("Cleanup failed");
      const stop = vi.fn(() => {
        if (kind === "throw") throw error;
        return Promise.reject(error);
      });
      vi.mocked(listen).mockResolvedValueOnce(stop);
      const release = claim();
      await flushEvents();
      release();
      release();
      await flushEvents();

      expect(stop).toHaveBeenCalledOnce();
      expect(console.error).toHaveBeenCalledExactlyOnceWith(error);
    },
  );

  it("handles native enable and disable failures", async () => {
    const error = new Error("Native command failed");
    vi.mocked(invoke).mockRejectedValue(error);
    const release = claim();
    release();
    await flushEvents();

    expect(console.error).toHaveBeenCalledTimes(2);
    expect(console.error).toHaveBeenNthCalledWith(1, error);
    expect(console.error).toHaveBeenNthCalledWith(2, error);
  });
});
