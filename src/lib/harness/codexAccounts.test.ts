import { beforeEach, describe, expect, it, vi } from "vitest";
const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
import {
  isQuotaWallError,
  isSharedScopeReached,
  pickFailoverAccount,
  quotaResetAt,
  respondCodexRefresh,
  resetCodexAccounts,
  type CodexAccount,
  accountCredentials,
  captureExternalAccount,
  failoverAccounts,
  loadCodexAccounts,
} from "./codexAccounts";
import { mapCodexNotification } from "./codexProtocol";

const account = (
  id: string,
  state: Partial<CodexAccount> = {},
): CodexAccount => ({
  id,
  accountId: id,
  email: `${id}@example.invalid`,
  hasTokens: true,
  addedAt: 1,
  ...state,
});
beforeEach(() => {
  invoke.mockReset();
  resetCodexAccounts();
});

describe("Codex quota walls", () => {
  it.each(["usageLimitExceeded", "rateLimitExceeded"])(
    "recognizes %s without message",
    (info) => {
      expect(isQuotaWallError(info)).toBe(true);
    },
  );
  it.each([
    "unauthorized",
    "other",
    { httpConnectionFailed: { httpStatusCode: 429 } },
  ])("does not override structured non-quota errors", (info) => {
    expect(isQuotaWallError(info, "quota 429 rate limit")).toBe(false);
  });
  it.each([
    "Usage limit reached",
    "rate-limit",
    "rate limit",
    "ratelimit",
    "QUOTA",
    "HTTP 429",
  ])("supports legacy %s", (message) => {
    expect(isQuotaWallError(undefined, message)).toBe(true);
    expect(isQuotaWallError(null, message)).toBe(true);
  });
  it("ignores unrelated legacy errors", () => {
    expect(isQuotaWallError(undefined, "unauthorized")).toBe(false);
    expect(isQuotaWallError(undefined)).toBe(false);
  });
  it.each([
    "workspace_owner_credits_depleted",
    "workspace_member_credits_depleted",
    "workspace_owner_usage_limit_reached",
    "workspace_member_usage_limit_reached",
  ])("gates shared scope %s", (scope) => {
    expect(isSharedScopeReached(scope)).toBe(true);
  });
  it("allows personal scope", () => {
    expect(isSharedScopeReached("rate_limit_reached")).toBe(false);
    expect(isSharedScopeReached(undefined)).toBe(false);
  });
  it("selects least recently used eligible distinct identity", () => {
    expect(
      pickFailoverAccount(
        [
          account("current"),
          account("alias", { accountId: "current" }),
          account("disabled", { disabledCause: "expired" }),
          account("blocked", { blockedUntilMs: 101 }),
          account("empty", { hasTokens: false }),
          account("recent", { lastUsedAt: 90 }),
          account("old", { lastUsedAt: 10, blockedUntilMs: 100 }),
        ],
        "current",
        100,
      )?.id,
    ).toBe("old");
    expect(
      pickFailoverAccount(
        [account("used", { lastUsedAt: 1 }), account("unused")],
        undefined,
        100,
      )?.id,
    ).toBe("unused");
    expect(pickFailoverAccount([], undefined)).toBeUndefined();
  });
  it("uses the latest exhausted window reset and a one-hour fallback", () => {
    expect(
      quotaResetAt(
        {
          primary: { usedPercent: 100, resetsAt: 20 },
          secondary: { usedPercent: 100, resetsAt: 30 },
        },
        1000,
      ),
    ).toBe(30000);
    expect(
      quotaResetAt({ primary: { usedPercent: 99, resetsAt: 20 } }, 1000),
    ).toBe(3601000);
  });
  it("passes terminal error info and retains snapshots without UI events", () => {
    expect(
      mapCodexNotification("turn/completed", {
        turn: {
          status: "failed",
          error: { message: "wall", codexErrorInfo: "usageLimitExceeded" },
        },
      }).turnCompleted,
    ).toEqual({
      status: "failed",
      error: "wall",
      codexErrorInfo: "usageLimitExceeded",
    });
    const snapshot = {
      primary: { usedPercent: 100, resetsAt: 20 },
      rateLimitReachedType: "rate_limit_reached",
    };
    expect(
      mapCodexNotification("account/rateLimits/updated", {
        rateLimits: snapshot,
      }),
    ).toEqual({ events: [], rateLimits: snapshot });
  });
});

it("coalesces same-account refreshes and keeps independent accounts parallel", async () => {
  let active = 0;
  let maxActive = 0;
  invoke.mockImplementation(async (command: string, args?: { id: string }) => {
    if (command === "codex_accounts_list")
      return [account("one"), account("two")];
    if (command === "codex_account_credentials")
      return {
        ...account(args!.id),
        accessToken: "fake-old",
        refreshToken: "fake-refresh",
      };
    if (command === "codex_account_refresh") {
      active++;
      maxActive = Math.max(active, maxActive);
      await Promise.resolve();
      active--;
      return {
        ...account(args!.id),
        accessToken: "fake-new",
        refreshToken: "fake-refresh",
      };
    }
  });
  const rpc = {
    respond: vi.fn(async () => {}),
    respondError: vi.fn(async () => {}),
  };
  await Promise.all([
    respondCodexRefresh(rpc, 1, { previousAccountId: "two" }, "one"),
    respondCodexRefresh(rpc, 2, { previousAccountId: "two" }, "one"),
    respondCodexRefresh(rpc, 3, {}, "one"),
  ]);
  // Two refreshes can overlap (different accounts), but account "two" shares
  // a single credential fetch across its two requests.
  expect(maxActive).toBe(2);
  expect(
    invoke.mock.calls.filter(
      ([command, args]) =>
        command === "codex_account_refresh" &&
        (args as { id: string }).id === "two",
    ),
  ).toHaveLength(1);
  expect(rpc.respond.mock.calls).toEqual([
    [1, { accessToken: "fake-new", chatgptAccountId: "two" }],
    [2, { accessToken: "fake-new", chatgptAccountId: "two" }],
    [3, { accessToken: "fake-new", chatgptAccountId: "one" }],
  ]);
  expect(rpc.respondError).not.toHaveBeenCalled();
});

it("makes one refresh attempt per request and returns a redacted error", async () => {
  invoke.mockImplementation(async (command: string) => {
    if (command === "codex_accounts_list") return [account("one")];
    if (command === "codex_account_credentials") return account("one");
    throw new Error("fake-private-endpoint-response");
  });
  const rpc = {
    respond: vi.fn(async () => {}),
    respondError: vi.fn(async () => {}),
  };
  await respondCodexRefresh(rpc, "refresh-id", {}, "one");
  expect(
    invoke.mock.calls.filter(
      ([command]) => command === "codex_account_refresh",
    ),
  ).toHaveLength(1);
  expect(rpc.respond).not.toHaveBeenCalled();
  expect(rpc.respondError).toHaveBeenCalledWith("refresh-id", {
    code: -32000,
    message: "Codex account refresh failed; sign in again",
  });
});

it("rereads external tokens at selection, refreshes expiry, and never inserts them into the pool", async () => {
  const jwt = (exp: number) => `fake.${btoa(JSON.stringify({ exp }))}.fake`;
  let auth = {
    accountId: "external-workspace",
    accessToken: jwt(Date.now() / 1000 + 3600),
    refreshToken: "fake-refresh",
  };
  invoke.mockImplementation(async (command: string) => {
    if (command === "codex_accounts_list") return [account("one")];
    if (command === "codex_auth_json_read") return { ...auth };
    if (command === "codex_auth_json_refresh")
      return { ...auth, accessToken: "fake-refreshed" };
    throw new Error("Unexpected credential command");
  });
  await loadCodexAccounts();
  await captureExternalAccount({
    request: async <T>() =>
      ({ account: { email: "external@example.invalid" } }) as T,
  });
  auth = { ...auth, accessToken: "fake-cli-refreshed" };
  expect((await accountCredentials("external")).accessToken).toBe(
    "fake-cli-refreshed",
  );
  auth = { ...auth, accessToken: jwt(1) };
  expect((await accountCredentials("external")).accessToken).toBe(
    "fake-refreshed",
  );
  expect(invoke).toHaveBeenCalledWith("codex_auth_json_refresh", {
    accountId: "external-workspace",
  });
  expect(failoverAccounts().find((a) => a.id === "external")?.email).toBe(
    "external@example.invalid",
  );
  expect(
    invoke.mock.calls.some(([command]) => command === "codex_account_upsert"),
  ).toBe(false);
  auth = { ...auth, accountId: "different-account" };
  await expect(accountCredentials("external")).rejects.toThrow(
    "account changed",
  );
});
