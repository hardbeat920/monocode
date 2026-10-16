import { beforeEach, describe, expect, it, vi } from "vitest";

const child = vi.hoisted(() => ({
  killChild: vi.fn(async () => undefined),
  spawnChild: vi.fn(async () => undefined),
  unwatchChild: vi.fn(),
  watchChild: vi.fn(),
  resolveClaudeBinary: vi.fn(async () => ({ path: "/bin/claude" })),
  resolveCodexBinary: vi.fn(async () => ({ path: "/bin/codex" })),
  resolveCursorBinary: vi.fn(async () => ({ path: "/bin/agent" })),
  resolveGrokBinary: vi.fn(async () => ({ path: "/bin/grok" })),
  resolveFxBinary: vi.fn(async () => ({ path: "/bin/fx" })),
}));

vi.mock("./child", () => child);
vi.mock("../../../platform/tauri/fs", () => ({ homeDir: vi.fn(async () => "/home/alice") }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ label: "test-window" }),
}));

import {
  harnessLoginArgs,
  isClaudeAuthFailureText,
  isHarnessAuthError,
  latestTurnHarnessAuthFailure,
  latestTurnNeedsHarnessLogin,
  loginHarness,
  resetHarnessLoginState,
  supportsHarnessLogin,
} from "./auth";

const CLAUDE_EXPIRED =
  "Failed to authenticate: OAuth session expired and could not be refreshed";

beforeEach(() => {
  resetHarnessLoginState();
  vi.clearAllMocks();
});

describe("harness login", () => {
  it("advertises only deterministic account login commands", () => {
    expect(harnessLoginArgs("claude")).toEqual(["auth", "login"]);
    expect(harnessLoginArgs("codex")).toEqual(["login"]);
    expect(harnessLoginArgs("cursor")).toEqual(["login"]);
    expect(harnessLoginArgs("grok")).toEqual(["login", "--oauth"]);
    expect(harnessLoginArgs("fx")).toEqual(["login", "vercel"]);
    expect(supportsHarnessLogin("opencode")).toBe(false);
    expect(supportsHarnessLogin("pi")).toBe(false);
    expect(supportsHarnessLogin("omp")).toBe(false);
  });

  it("recognizes provider authentication failures without matching unrelated errors", () => {
    expect(
      isHarnessAuthError(
        "Authentication required\n\nGrok Build is not signed in. Run `grok login` in a terminal.",
      ),
    ).toBe(true);
    expect(isHarnessAuthError("Claude sign-in expired")).toBe(true);
    expect(isHarnessAuthError("Provider request timed out")).toBe(false);
  });

  it("recognizes Claude Code's own sign-in failures", () => {
    for (const text of [
      CLAUDE_EXPIRED,
      "Invalid API key · Please run /login",
      "OAuth token revoked · Please run /login",
      'Failed to authenticate. API Error: 401 {"type":"error","error":{"type":"authentication_error","message":"OAuth token has expired."}}',
    ]) {
      expect(isHarnessAuthError(text), text).toBe(true);
      expect(isClaudeAuthFailureText(text), text).toBe(true);
    }
  });

  it("does not read a reply about authentication as a sign-in failure", () => {
    for (const text of [
      "Failed to authenticate errors usually mean the token in .env is stale.",
      'If you see "Failed to authenticate: OAuth session expired", run /login.',
      `${CLAUDE_EXPIRED}\n\nHere is how the refresh flow works in this repo.`,
      "The login handler returns 401 when the session cookie is missing.",
    ]) {
      expect(isClaudeAuthFailureText(text), text).toBe(false);
    }
  });

  it("treats a Claude sign-in failure as the turn's error only when it is the whole reply", () => {
    const user = { role: "user" as const, text: "Fix the build" };
    const reply = { role: "assistant" as const, text: CLAUDE_EXPIRED };
    expect(latestTurnHarnessAuthFailure([user, reply], "claude")).toBe(reply);
    expect(latestTurnNeedsHarnessLogin([user, reply], "claude")).toBe(true);
    // Other providers never fall back to reading reply text.
    expect(latestTurnNeedsHarnessLogin([user, reply], "codex")).toBe(false);
    expect(latestTurnNeedsHarnessLogin([user, reply])).toBe(false);
    // Still streaming, or one reply among other work, is not a failed turn.
    expect(
      latestTurnNeedsHarnessLogin(
        [user, { ...reply, streaming: true }],
        "claude",
      ),
    ).toBe(false);
    expect(
      latestTurnNeedsHarnessLogin(
        [user, { role: "tool", text: "Read package.json" }, reply],
        "claude",
      ),
    ).toBe(false);
    expect(
      latestTurnNeedsHarnessLogin(
        [user, { role: "assistant", text: "Looking at it." }, reply],
        "claude",
      ),
    ).toBe(false);
    // An earlier turn's failure does not carry over.
    expect(
      latestTurnNeedsHarnessLogin(
        [user, reply, { role: "user", text: "Try again" }],
        "claude",
      ),
    ).toBe(false);
    // Status rows do not count as output.
    expect(
      latestTurnNeedsHarnessLogin(
        [user, { role: "system", text: "Compacted context" }, reply],
        "claude",
      ),
    ).toBe(true);
  });

  it("only carries an authentication failure through its current turn", () => {
    const error = {
      role: "system" as const,
      notice: "error" as const,
      text: "Grok Build is not signed in.",
    };
    expect(latestTurnNeedsHarnessLogin([error])).toBe(true);
    expect(
      latestTurnNeedsHarnessLogin([
        error,
        { role: "user", text: "Retry this", notice: undefined },
      ]),
    ).toBe(false);
  });

  it("launches Claude's official login command and waits for success", async () => {
    const login = loginHarness("claude");
    await vi.waitFor(() => expect(child.watchChild).toHaveBeenCalledOnce());
    expect(child.spawnChild).toHaveBeenCalledWith(
      "monocode-provider-login-test-window-claude",
      "/bin/claude",
      ["auth", "login"],
      "/home/alice",
      undefined,
      "claude",
    );

    const onExit = child.watchChild.mock.calls[0]?.[2] as
      ((code: number | null) => void) | undefined;
    onExit?.(0);
    await expect(login).resolves.toBeUndefined();
  });

  it("isolates a named Codex account during sign-in", async () => {
    const login = loginHarness("codex", "account-work");
    await vi.waitFor(() => expect(child.watchChild).toHaveBeenCalledOnce());
    expect(child.spawnChild).toHaveBeenCalledWith(
      "monocode-provider-login-test-window-codex-account-work",
      "/bin/codex",
      ["login"],
      "/home/alice",
      { provider: "codex", id: "account-work" },
      "codex",
    );

    const onExit = child.watchChild.mock.calls[0]?.[2] as
      | ((code: number | null) => void)
      | undefined;
    onExit?.(0);
    await expect(login).resolves.toBeUndefined();
  });

  it("deduplicates repeated login clicks", async () => {
    const first = loginHarness("codex");
    const second = loginHarness("codex");
    expect(second).toBe(first);
    await vi.waitFor(() => expect(child.watchChild).toHaveBeenCalledOnce());

    const onExit = child.watchChild.mock.calls[0]?.[2] as
      ((code: number | null) => void) | undefined;
    onExit?.(0);
    await first;
    expect(child.spawnChild).toHaveBeenCalledOnce();
  });

  it("does not invent one login flow for multi-provider harnesses", async () => {
    await expect(loginHarness("opencode")).rejects.toThrow(
      "does not offer a single browser sign-in flow",
    );
    expect(child.spawnChild).not.toHaveBeenCalled();
  });
});
