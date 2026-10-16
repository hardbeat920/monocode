import { beforeEach, describe, expect, it, vi } from "vitest";

const auth = vi.hoisted(() => ({
  loginHarness:
    vi.fn<(_harness: string, _accountId?: string) => Promise<void>>(),
}));
const rateLimitsFetch = vi.hoisted(() => ({
  fetchClaudeRateLimits: vi.fn(),
  fetchCodexRateLimits: vi.fn(),
  fetchDevinRateLimits: vi.fn(),
  fetchOpencodeGoRateLimits: vi.fn(),
}));

vi.mock("../../../integrations/harness/core/auth", () => auth);
vi.mock("./rateLimitsFetch", () => rateLimitsFetch);

import type { ProviderRateLimits } from "./rateLimits";
import { clearCachedRateLimits, getCachedRateLimits } from "./rateLimitsCache";
import { signInProvider } from "./providerSignIn";

function limits(status: "ok" | "error", error: string | null = null) {
  return {
    provider: "claude",
    session: null,
    weekly: null,
    monthly: null,
    resetCredits: null,
    updatedAt: Date.now(),
    error,
    status,
  } satisfies ProviderRateLimits;
}

beforeEach(() => {
  vi.clearAllMocks();
  clearCachedRateLimits();
  auth.loginHarness.mockResolvedValue();
});

describe("signInProvider", () => {
  it("signs in the conversation's account and confirms it with fresh usage", async () => {
    rateLimitsFetch.fetchClaudeRateLimits.mockResolvedValue(limits("ok"));
    await signInProvider("claude", "account-work");
    expect(auth.loginHarness).toHaveBeenCalledWith("claude", "account-work");
    expect(rateLimitsFetch.fetchClaudeRateLimits).toHaveBeenCalledWith(
      "account-work",
    );
    expect(getCachedRateLimits("claude", "account-work").status).toBe("ok");
  });

  it("fails when the provider still reports the account signed out", async () => {
    rateLimitsFetch.fetchClaudeRateLimits.mockResolvedValue(
      limits("error", "Claude Code is not signed in"),
    );
    await expect(signInProvider("claude")).rejects.toThrow(
      "Claude Code is not signed in",
    );
    expect(auth.loginHarness).toHaveBeenCalledWith("claude");
    expect(getCachedRateLimits("claude").status).toBe("error");
  });

  it("only runs the login for a provider without usage readings", async () => {
    await signInProvider("grok");
    expect(auth.loginHarness).toHaveBeenCalledWith("grok");
    expect(rateLimitsFetch.fetchClaudeRateLimits).not.toHaveBeenCalled();
  });
});
