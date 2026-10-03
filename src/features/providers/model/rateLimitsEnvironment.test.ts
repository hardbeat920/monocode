import { beforeEach, describe, expect, it, vi } from "vitest";
const localFetch = vi.hoisted(() => vi.fn());
vi.mock("./rateLimitsFetch", () => ({ fetchDroidRateLimits: localFetch }));
import {
  clearCachedRateLimits,
  getCachedRateLimits,
  loadRateLimits,
  setCachedRateLimits,
} from "./rateLimitsCache";
import { unavailableRateLimits } from "./rateLimits";

describe("remote Droid usage", () => {
  beforeEach(() => {
    clearCachedRateLimits();
    localFetch.mockClear();
  });
  it("does not show or fetch desktop usage for a remote session", async () => {
    const desktop = unavailableRateLimits("droid", "Desktop account");
    setCachedRateLimits("droid", "default", desktop);
    const remote = await loadRateLimits("droid", "default", false, "host-b");
    expect(localFetch).not.toHaveBeenCalled();
    expect(remote.error).toBe("Usage is unavailable for remote sessions");
    expect(remote.session).toBeNull();
    expect(getCachedRateLimits("droid")).toBe(desktop);
    expect(getCachedRateLimits("droid", "default", "host-b")).toBe(remote);
    expect(
      getCachedRateLimits("droid", "another-account", "host-b").status,
    ).toBe("idle");
    expect(getCachedRateLimits("droid", "default", "host-c").status).toBe(
      "idle",
    );
  });
});
