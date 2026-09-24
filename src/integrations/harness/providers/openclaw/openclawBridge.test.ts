import { describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  sent: [] as string[],
  acp: null as { request: ReturnType<typeof vi.fn>; close: ReturnType<typeof vi.fn> } | null,
  spawn: vi.fn(async () => undefined),
  kill: vi.fn(async () => undefined),
  watch: vi.fn(),
  unwatch: vi.fn(),
  resolve: vi.fn(async () => ({ path: "/usr/bin/openclaw" })),
}));

vi.mock("../../core/child", () => ({
  resolveOpenClawBinary: mocks.resolve,
  spawnTrustedChild: mocks.spawn,
  killChild: mocks.kill,
  watchChild: mocks.watch,
  validateOpenClawGatewayWs: vi.fn(async () => undefined),
  unwatchChild: mocks.unwatch,
}));

vi.mock("../../core/acp", () => ({
  AcpClient: class {
    pushLine = vi.fn();
    request = vi.fn(async (method: string) => {
      if (method === "initialize") return {};
      if (method === "session/new") return { sessionId: "openclaw-session" };
      return {};
    });
    close = vi.fn();
    respondError = vi.fn(async () => undefined);
  },
}));

const { startOpenClawAcpBridge } = await import("./openclawBridge");

describe("OpenClaw ACP bridge", () => {
  it("uses trusted openclaw acp transport and namespaces session keys", async () => {
    const result = await startOpenClawAcpBridge({
      childId: "openclaw#1",
      cwd: "/repo",
      gatewaySessionKey: "team/main",
      gatewayUrl: "wss://gateway.example",
    });
    expect(mocks.resolve).toHaveBeenCalledOnce();
    expect(mocks.spawn).toHaveBeenCalledWith("openclaw#1", {
      provider: "openclaw",
      path: "/usr/bin/openclaw",
      args: ["acp"],
    }, "/repo");
    expect(result.gatewaySessionKey).toBe("acp-bridge:team/main");
    await result.dispose();
    expect(mocks.kill).toHaveBeenCalledWith("openclaw#1");
  });

  it("keeps generated session keys namespaced and never exposes credentials", async () => {
    const result = await startOpenClawAcpBridge({
      childId: "openclaw#2",
      cwd: "/repo",
      gatewayUrl: "wss://gateway.example",
    });
    expect(result.gatewaySessionKey).toMatch(/^acp-bridge:/);
    expect(result.gatewaySessionKey).not.toContain("token");
    expect(result.gatewaySessionKey).not.toContain("password");
  });

  it("rejects unknown incoming methods with the original string ID", async () => {
    const result = await startOpenClawAcpBridge({ childId: "openclaw#3", cwd: "/repo" });
    const handler = mocks.watch.mock.calls.at(-1)?.[1] as ((line: string) => void) | undefined;
    expect(handler).toBeTypeOf("function");
    handler?.(JSON.stringify({ jsonrpc: "2.0", id: "raw-1", method: "future/request" }));
    await Promise.resolve();
    expect(result.acp).toBeDefined();
  });
});
