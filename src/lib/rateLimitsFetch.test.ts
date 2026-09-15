vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("./fs", () => ({ homeDir: vi.fn() }));
vi.mock("./harness/child", () => ({
  killChild: vi.fn(),
  resolveCodexBinary: vi.fn(),
  spawnChild: vi.fn(),
  unwatchChild: vi.fn(),
  watchChild: vi.fn(),
}));

import { invoke } from "@tauri-apps/api/core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { fetchClaudeRateLimits } from "./rateLimitsFetch";

const mockedInvoke = vi.mocked(invoke);
const OK_BODY = JSON.stringify({
  five_hour: { utilization: 6, resets_at: "2026-09-13T16:50:00Z" },
  seven_day: { utilization: 28, resets_at: "2026-09-15T08:00:00Z" },
});

let now = 1_000_000;

beforeEach(() => {
  vi.spyOn(Date, "now").mockImplementation(() => now);
  mockedInvoke.mockReset();
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("fetchClaudeRateLimits", () => {
  it("reuses a snapshot younger than maxAgeMs and refetches after", async () => {
    mockedInvoke.mockResolvedValue({ status: "ok", body: OK_BODY });
    await fetchClaudeRateLimits();
    now += 10_000;
    await fetchClaudeRateLimits({ maxAgeMs: 30_000 });
    expect(mockedInvoke).toHaveBeenCalledTimes(1);
    now += 30_000;
    await fetchClaudeRateLimits({ maxAgeMs: 30_000 });
    expect(mockedInvoke).toHaveBeenCalledTimes(2);
  });

  it("expires an unavailable snapshot so a later sign-in is seen", async () => {
    mockedInvoke.mockResolvedValueOnce({
      status: "unavailable",
      error: "Claude not signed in",
    });
    expect((await fetchClaudeRateLimits()).status).toBe("unavailable");
    now += 60_000;
    mockedInvoke.mockResolvedValueOnce({ status: "ok", body: OK_BODY });
    const next = await fetchClaudeRateLimits({ maxAgeMs: 30_000 });
    expect(next.status).toBe("ok");
    expect(next.session?.usedPercent).toBe(6);
  });

  it("keeps the last windows when a fetch fails", async () => {
    mockedInvoke.mockResolvedValueOnce({ status: "ok", body: OK_BODY });
    await fetchClaudeRateLimits();
    mockedInvoke.mockRejectedValueOnce(new Error("offline"));
    const failed = await fetchClaudeRateLimits();
    expect(failed.status).toBe("error");
    expect(failed.error).toBe("offline");
    expect(failed.weekly?.usedPercent).toBe(28);
  });
});
