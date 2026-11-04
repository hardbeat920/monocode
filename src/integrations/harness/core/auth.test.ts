import { beforeEach, describe, expect, it, vi } from "vitest";

const child = vi.hoisted(() => ({
  killChild: vi.fn(async () => undefined),
  spawnChild: vi.fn(async () => undefined),
  writeChild: vi.fn(async () => undefined),
  unwatchChild: vi.fn(),
  watchChild: vi.fn(),
  resolveClaudeBinary: vi.fn(async () => ({ path: "/bin/claude" })),
  resolveCodexBinary: vi.fn(async () => ({ path: "/bin/codex" })),
  resolveCursorBinary: vi.fn(async () => ({ path: "/bin/agent" })),
  resolveGrokBinary: vi.fn(async () => ({ path: "/bin/grok" })),
  resolveFxBinary: vi.fn(async () => ({ path: "/bin/fx" })),
}));

vi.mock("./child", () => child);
vi.mock("../../../platform/tauri/fs", () => ({
  homeDir: vi.fn(async () => "/home/alice"),
}));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ label: "test-window" }),
}));

import {
  harnessLoginArgs,
  isHarnessAuthError,
  latestTurnNeedsHarnessLogin,
  loginHarness,
  submitHarnessLoginCode,
  resetHarnessLoginState,
  supportsHarnessLogin,
} from "./auth";

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
      ((code: number | null) => void) | undefined;
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

  it("cancels a browser-closed login and permits a fresh attempt", async () => {
    const controller = new AbortController();
    const first = loginHarness("claude", undefined, {
      signal: controller.signal,
    });
    await vi.waitFor(() => expect(child.spawnChild).toHaveBeenCalledOnce());
    controller.abort();
    await expect(first).rejects.toThrow("cancelled");
    expect(child.killChild).toHaveBeenCalledTimes(2);
    const second = loginHarness("claude");
    await vi.waitFor(() => expect(child.spawnChild).toHaveBeenCalledTimes(2));
    (child.watchChild.mock.calls[1]?.[2] as (code: number) => void)(0);
    await expect(second).resolves.toBeUndefined();
  });

  it("does not kill another caller's shared login when the dialog cancels", async () => {
    const controller = new AbortController();
    const first = loginHarness("claude", undefined, {
      signal: controller.signal,
    });
    const shared = loginHarness("claude");
    await vi.waitFor(() => expect(child.spawnChild).toHaveBeenCalledOnce());
    controller.abort();
    await expect(first).rejects.toThrow("cancelled");
    expect(child.killChild).toHaveBeenCalledTimes(1);
    (child.watchChild.mock.calls[0]?.[2] as (code: number) => void)(0);
    await expect(shared).resolves.toBeUndefined();
  });

  it("does not spawn after cancellation during binary resolution", async () => {
    let resolveBinary!: (value: { path: string }) => void;
    child.resolveClaudeBinary.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveBinary = resolve;
        }),
    );
    const controller = new AbortController();
    const login = loginHarness("claude", undefined, {
      signal: controller.signal,
    });
    controller.abort();
    resolveBinary({ path: "/bin/claude" });
    await expect(login).rejects.toThrow("cancelled");
    expect(child.spawnChild).not.toHaveBeenCalled();
  });

  it("settles cancellation before a stuck resolver and ignores its late result after retry", async () => {
    let resolveOld!: (value: { path: string }) => void;
    child.resolveClaudeBinary.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveOld = resolve;
        }),
    );
    const controller = new AbortController();
    const first = loginHarness("claude", undefined, {
      signal: controller.signal,
    });
    controller.abort();
    const prompt = await Promise.race([
      first.then(
        () => true,
        () => true,
      ),
      new Promise<false>((resolve) => setTimeout(() => resolve(false), 50)),
    ]);
    const retry = loginHarness("claude");
    if (!prompt) {
      resolveOld({ path: "/bin/old-claude" });
      await Promise.allSettled([first, retry]);
      expect(prompt).toBe(true);
    }
    await vi.waitFor(() => expect(child.spawnChild).toHaveBeenCalledOnce());
    resolveOld({ path: "/bin/old-claude" });
    await Promise.resolve();
    expect(child.spawnChild.mock.calls[0]?.[1]).toBe("/bin/claude");
    expect(child.killChild).toHaveBeenCalledOnce();
    (child.watchChild.mock.calls[0]?.[2] as (code: number) => void)(0);
    await expect(retry).resolves.toBeUndefined();
    await expect(first).rejects.toThrow("cancelled");
  });

  it("waits for an in-progress spawn before killing and retrying", async () => {
    let finishSpawn!: () => void;
    child.spawnChild.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finishSpawn = resolve;
        }),
    );
    const controller = new AbortController();
    const first = loginHarness("claude", undefined, {
      signal: controller.signal,
    });
    await vi.waitFor(() => expect(child.spawnChild).toHaveBeenCalledOnce());
    controller.abort();
    finishSpawn();
    await expect(first).rejects.toThrow("cancelled");
    const second = loginHarness("claude");
    await vi.waitFor(() => expect(child.spawnChild).toHaveBeenCalledTimes(2));
    (child.watchChild.mock.calls[1]?.[2] as (code: number) => void)(0);
    await second;
  });

  it("exposes Claude's printed URL and accepts a pasted code only for its owner", async () => {
    const controller = new AbortController();
    const onUrl = vi.fn();
    const login = loginHarness("claude", undefined, {
      signal: controller.signal,
      onUrl,
    });
    await vi.waitFor(() => expect(child.watchChild).toHaveBeenCalledOnce());
    (child.watchChild.mock.calls[0]?.[1] as (line: string) => void)(
      "If the browser didn't open, visit: https://claude.ai/oauth/authorize?state=private",
    );
    expect(onUrl).toHaveBeenCalledWith(
      "https://claude.ai/oauth/authorize?state=private",
    );
    (child.watchChild.mock.calls[0]?.[1] as (line: string) => void)(
      "If the browser didn't open, visit: https://claude.ai.evil.example/steal",
    );
    expect(onUrl).toHaveBeenCalledOnce();
    await expect(
      submitHarnessLoginCode(
        "claude",
        "sample-code",
        new AbortController().signal,
      ),
    ).rejects.toThrow("no longer active");
    await submitHarnessLoginCode("claude", "sample-code", controller.signal);
    expect(child.writeChild).toHaveBeenCalledWith(
      "monocode-provider-login-test-window-claude",
      "sample-code",
    );
    (child.watchChild.mock.calls[0]?.[2] as (code: number) => void)(0);
    await login;
  });

  it("routes a named Claude account's code to its own child", async () => {
    const controller = new AbortController();
    const login = loginHarness("claude", "account-work", {
      signal: controller.signal,
    });
    await vi.waitFor(() => expect(child.spawnChild).toHaveBeenCalledOnce());
    await expect(
      submitHarnessLoginCode("claude", "sample-code", controller.signal),
    ).rejects.toThrow("no longer active");
    await submitHarnessLoginCode(
      "claude",
      "sample-code",
      controller.signal,
      "account-work",
    );
    expect(child.writeChild).toHaveBeenCalledWith(
      "monocode-provider-login-test-window-claude-account-work",
      "sample-code",
    );
    (child.watchChild.mock.calls[0]?.[2] as (code: number) => void)(0);
    await login;
  });

  it("does not invent one login flow for multi-provider harnesses", async () => {
    await expect(loginHarness("opencode")).rejects.toThrow(
      "does not offer a single browser sign-in flow",
    );
    expect(child.spawnChild).not.toHaveBeenCalled();
  });
});
