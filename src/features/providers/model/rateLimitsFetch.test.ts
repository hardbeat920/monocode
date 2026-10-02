import { beforeEach, describe, expect, it, vi } from "vitest";

const child = vi.hoisted(() => ({
  live: 0,
  maxLive: 0,
  spawned: [] as string[],
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("../../../platform/tauri/fs", () => ({
  homeDir: async () => "/home/test",
}));
vi.mock("../../../integrations/harness/core/child", () => ({
  resolveCodexBinary: async () => ({ path: "/bin/codex" }),
  spawnChild: async (
    _id: string,
    _path: string,
    _args: string[],
    _cwd: string,
    account: { id: string },
  ) => {
    child.live += 1;
    child.maxLive = Math.max(child.maxLive, child.live);
    child.spawned.push(account.id);
  },
  watchChild: () => undefined,
  unwatchChild: () => {
    child.live = Math.max(0, child.live - 1);
  },
  killChild: async () => undefined,
}));
vi.mock("../../../integrations/harness/core/jsonRpc", () => ({
  JsonRpcClient: class {
    close() {}
    pushLine() {}
    respond() {
      return Promise.resolve();
    }
    notify() {
      return Promise.resolve();
    }
    async request(method: string) {
      if (method !== "account/rateLimits/read") return {};
      await new Promise((resolve) => setTimeout(resolve, 5));
      return {
        rateLimits: {
          primary: { usedPercent: 10, windowDurationMins: 300 },
        },
      };
    }
  },
}));

import { invoke } from "@tauri-apps/api/core";
import {
  consumeClaudeRateLimitResetCredit,
  fetchCodexRateLimits,
} from "./rateLimitsFetch";

describe("fetchCodexRateLimits", () => {
  beforeEach(() => {
    child.live = 0;
    child.maxLive = 0;
    child.spawned = [];
  });

  it("runs usage probes for different accounts one at a time", async () => {
    const results = await Promise.all([
      fetchCodexRateLimits("default"),
      fetchCodexRateLimits("account-work"),
      fetchCodexRateLimits("account-personal"),
    ]);

    expect(child.maxLive).toBe(1);
    expect(child.spawned).toEqual([
      "default",
      "account-work",
      "account-personal",
    ]);
    expect(results.map((result) => result.session?.usedPercent)).toEqual([
      10, 10, 10,
    ]);
  });
});

describe("Claude reset redemption", () => {
  beforeEach(() => vi.mocked(invoke).mockReset());
  it("binds the request to the chosen account and exact offer", async () => {
    vi.mocked(invoke).mockResolvedValue("reset");
    expect(
      await consumeClaudeRateLimitResetCredit(
        "cedar_ember:promo",
        "account-work",
      ),
    ).toBe("reset");
    expect(invoke).toHaveBeenCalledExactlyOnceWith(
      "consume_claude_rate_limit_reset",
      {
        accountId: "account-work",
        creditId: "cedar_ember:promo",
        requestId: expect.any(String),
      },
    );
  });
  it("retains an idempotency key after uncertainty, isolates accounts, and releases it after a conclusive result", async () => {
    vi.mocked(invoke)
      .mockRejectedValueOnce("Claude reset was not confirmed")
      .mockResolvedValue("reset");
    await expect(
      consumeClaudeRateLimitResetCredit("cedar_ember:retry", "account-a"),
    ).rejects.toThrow("not confirmed");
    expect(invoke).toHaveBeenCalledTimes(1);
    await consumeClaudeRateLimitResetCredit("cedar_ember:retry", "account-b");
    await consumeClaudeRateLimitResetCredit("cedar_ember:retry", "account-a");
    await consumeClaudeRateLimitResetCredit("cedar_ember:retry", "account-a");
    const args = vi
      .mocked(invoke)
      .mock.calls.map((call) => call[1] as { requestId: string });
    expect(args[0].requestId).toBe(args[2].requestId);
    expect(args[1].requestId).not.toBe(args[0].requestId);
    expect(args[3].requestId).not.toBe(args[0].requestId);
  });
  it("does not invent success from unknown results or automatically select an offer", async () => {
    vi.mocked(invoke).mockResolvedValue({ result: "reset" });
    await expect(
      consumeClaudeRateLimitResetCredit("juniper_tide"),
    ).rejects.toThrow("not confirmed");
    vi.mocked(invoke).mockClear();
    await expect(consumeClaudeRateLimitResetCredit(undefined)).rejects.toThrow(
      "Select",
    );
    expect(invoke).not.toHaveBeenCalled();
  });
});
