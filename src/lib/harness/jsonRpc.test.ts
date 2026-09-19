import { beforeEach, describe, expect, it, vi } from "vitest";

const transport = vi.hoisted(() => ({
  onWrite: async (_sessionId: string, _line: string): Promise<void> => {},
}));

vi.mock("./child", () => ({
  writeChild: (sessionId: string, line: string) =>
    transport.onWrite(sessionId, line),
}));

import { JsonRpcClient } from "./jsonRpc";

describe("JsonRpcClient", () => {
  beforeEach(() => {
    transport.onWrite = async () => {};
  });

  it("accepts a response delivered before the write resolves", async () => {
    let client!: JsonRpcClient;
    transport.onWrite = async (_sessionId, line) => {
      const outbound = JSON.parse(line) as { id: number };
      client.pushLine(
        JSON.stringify({
          jsonrpc: "2.0",
          id: outbound.id,
          result: { ok: true },
        }),
      );
    };
    client = new JsonRpcClient("fast", {});

    await expect(client.request("session/set_mode")).resolves.toEqual({
      ok: true,
    });
  });

  it("rejects and removes a request when writing fails", async () => {
    transport.onWrite = async () => {
      throw new Error("pipe closed");
    };
    const client = new JsonRpcClient("failed", {});

    await expect(client.request("initialize")).rejects.toThrow("pipe closed");
  });

  it("bounds a blocked write instead of outliving the request deadline", async () => {
    vi.useFakeTimers();
    try {
      transport.onWrite = () => new Promise<void>(() => undefined);
      const client = new JsonRpcClient("wedged", {});
      const outcome = client.request("initialize", undefined, 60_000).then(
        () => "resolved",
        (e: Error) => e.message,
      );
      // The 15s write bound fires long before the request's own 60s deadline.
      await vi.advanceTimersByTimeAsync(16_000);
      await expect(outcome).resolves.toMatch(/timed out/);
    } finally {
      vi.useRealTimers();
    }
  });

  it("does not let a slow request deadline report as unhandled while the write is pending", async () => {
    vi.useFakeTimers();
    const unhandled: unknown[] = [];
    const onUnhandled = (reason: unknown) => unhandled.push(reason);
    process.on("unhandledRejection", onUnhandled);
    try {
      transport.onWrite = () => new Promise<void>(() => undefined);
      const client = new JsonRpcClient("quiet", {});
      const request = client.request("initialize", undefined, 5_000);
      const settled = request.catch((e: Error) => e.message);
      // At 5s the request's own deadline fires while the write stays blocked;
      // the outer promise only settles once the write bound returns it at 15s.
      await vi.advanceTimersByTimeAsync(6_000);
      await vi.advanceTimersByTimeAsync(15_000);
      await expect(settled).resolves.toMatch(/timed out/);
      expect(unhandled).toEqual([]);
    } finally {
      process.off("unhandledRejection", onUnhandled);
      vi.useRealTimers();
    }
  });
});
