// @vitest-environment happy-dom

import { afterEach, describe, expect, it, vi } from "vitest";
import {
  applyPreparedNewThreadBackground,
  clearPreparedNewThreadBackground,
  prepareNewThreadBackgroundEffect,
} from "./newThreadBackgroundEffects";

afterEach(() => {
  clearPreparedNewThreadBackground();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("new-thread background effects", () => {
  it("uses the original asset directly for None so animation is preserved", async () => {
    const fetch = vi.fn();
    const worker = vi.fn(() => {
      throw new Error("None must not start a worker");
    });
    vi.stubGlobal("fetch", fetch);
    vi.stubGlobal("Worker", worker);
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
      callback(0);
      return 1;
    });

    await applyPreparedNewThreadBackground(
      "/background.gif?v=101",
      "asset://localhost/background.gif?v=101",
      "none",
      false,
    );

    expect(fetch).not.toHaveBeenCalled();
    expect(worker).not.toHaveBeenCalled();
    expect(
      document.documentElement.style.getPropertyValue(
        "--chat-background-image",
      ),
    ).toContain("background.gif?v=101");
    expect(document.documentElement.classList).toContain(
      "chat-background-effect-ready",
    );
  });

  it("drops rejected source promises so transient failures can retry", async () => {
    const fetch = vi.fn().mockRejectedValue(new Error("temporary failure"));
    vi.stubGlobal("fetch", fetch);
    const sourceKey = `/background.png?v=${Date.now()}`;

    await expect(
      prepareNewThreadBackgroundEffect(
        sourceKey,
        "asset://localhost/background.png",
        "dither",
        false,
      ),
    ).rejects.toThrow("temporary failure");
    await expect(
      prepareNewThreadBackgroundEffect(
        sourceKey,
        "asset://localhost/background.png",
        "dither",
        false,
      ),
    ).rejects.toThrow("temporary failure");

    expect(fetch).toHaveBeenCalledTimes(2);
  });
});
