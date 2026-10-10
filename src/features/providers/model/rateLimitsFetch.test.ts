import { beforeEach, describe, expect, it, vi } from "vitest";

const child = vi.hoisted(() => ({
  live: 0,
  maxLive: 0,
  spawned: [] as string[],
  codexResult: undefined as unknown,
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("../../../platform/tauri/fs", () => ({
  homeDir: async () => "/home/test",
}));
vi.mock("../../../integrations/harness/core/child", () => ({
  resolveCodexBinary: async () => ({ path: "/bin/codex" }),
  inspectHarnessBinary: async () => ({
    path: "/bin/devin",
    version: "devin 3000.6.7 (260a97c8)",
  }),
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
      if (child.codexResult !== undefined) return child.codexResult;
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
  cachedDevinIdentity,
  CLAUDE_STALE_TOKEN_MESSAGE,
  fetchClaudeRateLimits,
  fetchCodexRateLimits,
  fetchDevinRateLimits,
} from "./rateLimitsFetch";

describe("fetchDevinRateLimits", () => {
  const signedIn = {
    status: "ok",
    httpStatus: 200,
    usage: { name: "Ada", email: "ada@example.com", plan: "Pro", weeklyRemainingPercent: 42 },
  };

  it("sends the installed CLI's version and keeps the identity it returns", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(signedIn);
    const limits = await fetchDevinRateLimits();
    expect(invoke).toHaveBeenLastCalledWith("fetch_devin_usage", { cliVersion: "3000.6.7" });
    expect(limits.weekly?.usedPercent).toBe(58);
    expect(cachedDevinIdentity()).toEqual({ name: "Ada", email: "ada@example.com", plan: "Pro" });
  });

  it("forgets the identity when the key is rejected, not on a network failure", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(signedIn);
    await fetchDevinRateLimits();
    vi.mocked(invoke).mockResolvedValueOnce({ status: "error", error: "Devin usage request failed: timeout" });
    await fetchDevinRateLimits();
    expect(cachedDevinIdentity()?.email).toBe("ada@example.com");
    vi.mocked(invoke).mockResolvedValueOnce({
      status: "error",
      httpStatus: 401,
      error: "Devin sign-in expired",
    });
    const limits = await fetchDevinRateLimits();
    expect(limits.status).toBe("error");
    expect(cachedDevinIdentity()).toBeNull();
  });
});

describe("fetchCodexRateLimits", () => {
  beforeEach(() => {
    child.live = 0;
    child.maxLive = 0;
    child.spawned = [];
    child.codexResult = undefined;
  });

  it("reports a non-object response as an error, not an empty ok", async () => {
    child.codexResult = "garbage";
    const limits = await fetchCodexRateLimits("default");
    expect(limits.status).toBe("error");
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

describe("fetchClaudeRateLimits", () => {
  it("reports a lapsed stored token without asking to sign in again", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ status: "stale" });
    const limits = await fetchClaudeRateLimits();
    expect(limits.status).toBe("error");
    expect(limits.error).toBe(CLAUDE_STALE_TOKEN_MESSAGE);
    expect(limits.error).not.toMatch(/expired|sign-in|not signed in/i);
  });

  it("treats a 200 without usage windows as an error", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ status: "ok", body: "{}" });
    const limits = await fetchClaudeRateLimits();
    expect(limits.status).toBe("error");
  });
});
