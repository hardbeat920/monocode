import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mock = vi.hoisted(() => ({
  onLine: undefined as ((line: string) => void) | undefined,
  onExit: undefined as ((code: number | null) => void) | undefined,
  sent: [] as { id: number; method: string; params: unknown }[],
  methods: [{ id: "oauth-personal" }],
  failAuth: false,
  holdAuth: false,
  spawn: vi.fn(async () => undefined),
  kill: vi.fn(async () => undefined),
  unwatch: vi.fn(),
}));

vi.mock("../../core/child", () => ({
  resolveAntigravityBinary: async () => ({
    path: "/opt/agy/agy_acp_server.par",
    args: ["--uid="],
  }),
  spawnChild: mock.spawn,
  killChild: mock.kill,
  unwatchChild: mock.unwatch,
  watchChild: (
    _id: string,
    onLine: typeof mock.onLine,
    onExit: typeof mock.onExit,
  ) => {
    mock.onLine = onLine;
    mock.onExit = onExit;
  },
  writeChild: async (_id: string, line: string) => {
    const message = JSON.parse(line);
    mock.sent.push(message);
    if (message.method === "authenticate" && mock.holdAuth) return;
    queueMicrotask(() =>
      mock.onLine?.(
        JSON.stringify({
          jsonrpc: "2.0",
          id: message.id,
          ...(message.method === "authenticate" && mock.failAuth
            ? { error: { code: -32000, message: "Google sign-in failed" } }
            : {
                result:
                  message.method === "initialize"
                    ? { authMethods: mock.methods }
                    : {},
              }),
        }),
      ),
    );
  },
}));

import { loginAntigravity } from "./antigravityAuth";

beforeEach(() => {
  vi.clearAllMocks();
  mock.sent.length = 0;
  mock.methods = [{ id: "oauth-personal" }];
  mock.failAuth = false;
  mock.holdAuth = false;
});
afterEach(() => vi.useRealTimers());

describe("Antigravity ACP sign-in", () => {
  it("initializes and authenticates the official server, then cleans up", async () => {
    await loginAntigravity("login");
    expect(mock.spawn).toHaveBeenCalledWith(
      "login",
      "/opt/agy/agy_acp_server.par",
      ["--uid="],
      "/opt/agy/",
      undefined,
      "antigravity",
    );
    expect(mock.sent.map((message) => message.method)).toEqual([
      "initialize",
      "authenticate",
    ]);
    expect(mock.sent[1].params).toEqual({ methodId: "oauth-personal" });
    expect(mock.unwatch).toHaveBeenCalledWith("login");
    expect(mock.kill).toHaveBeenLastCalledWith("login");
  });

  it("reports authentication rejection and cleans up the child", async () => {
    mock.failAuth = true;
    await expect(loginAntigravity("login")).rejects.toThrow(
      "Google sign-in failed",
    );
    expect(mock.unwatch).toHaveBeenCalledWith("login");
    expect(mock.kill).toHaveBeenCalledTimes(2);
  });

  it("does not select an authentication method the server does not advertise", async () => {
    mock.methods = [{ id: "agent-platform" }];
    await expect(loginAntigravity("login")).rejects.toThrow(
      "does not offer Google account sign-in",
    );
    expect(mock.sent.map((message) => message.method)).toEqual(["initialize"]);
    expect(mock.kill).toHaveBeenCalledTimes(2);
  });

  it("rejects if the process exits while browser authentication is pending", async () => {
    mock.holdAuth = true;
    const login = loginAntigravity("login");
    const failure = expect(login).rejects.toThrow("exited before completing");
    await vi.waitFor(() => expect(mock.sent).toHaveLength(2));
    mock.onExit?.(1);
    await failure;
    expect(mock.unwatch).toHaveBeenCalledWith("login");
  });

  it("bounds browser login and terminates a timed-out child", async () => {
    vi.useFakeTimers();
    mock.holdAuth = true;
    const login = loginAntigravity("login");
    const failure = expect(login).rejects.toThrow("authenticate timed out");
    await vi.advanceTimersByTimeAsync(10 * 60_000 + 1);
    await failure;
    expect(mock.unwatch).toHaveBeenCalledWith("login");
    expect(mock.kill).toHaveBeenCalledTimes(2);
  });
});
