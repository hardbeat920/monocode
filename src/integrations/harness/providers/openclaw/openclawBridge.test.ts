import { describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
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
  unwatchChild: mocks.unwatch,
}));

vi.mock("../../core/acp", () => ({
  AcpClient: class {
    request = vi.fn(async (method: string) => {
      if (method === "initialize") return {};
      return { sessionId: "openclaw-session" };
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
  });
});
