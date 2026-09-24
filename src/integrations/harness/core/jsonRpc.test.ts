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

  it("correlates out-of-order responses and ignores unknown notifications", async () => {
    let client!: JsonRpcClient;
    const requests: { id: number; method: string }[] = [];
    transport.onWrite = async (_sessionId, line) => {
      const outbound = JSON.parse(line) as { id: number; method: string };
      requests.push(outbound);
    };
    const notifications: string[] = [];
    client = new JsonRpcClient("out-of-order", {
      onNotification: (method) => notifications.push(method),
    });

    const first = client.request("session/new");
    const second = client.request("session/prompt");
    await Promise.resolve();
    expect(requests).toHaveLength(2);

    client.pushLine(JSON.stringify({ jsonrpc: "2.0", method: "unknown" }));
    client.pushLine(JSON.stringify({
      jsonrpc: "2.0",
      method: "session/update",
      params: { text: "chunk" },
    }));
    client.pushLine(JSON.stringify({
      jsonrpc: "2.0",
      id: requests[1].id,
      result: { stopReason: "end_turn" },
    }));
    client.pushLine(JSON.stringify({
      jsonrpc: "2.0",
      id: requests[0].id,
      result: { sessionId: "session-1" },
    }));

    await expect(first).resolves.toEqual({ sessionId: "session-1" });
    await expect(second).resolves.toEqual({ stopReason: "end_turn" });
    expect(notifications).toEqual(["unknown", "session/update"]);
  });

  it("dispatches inbound requests and supports string response ids", async () => {
    let client!: JsonRpcClient;
    const inbound: Promise<void>[] = [];
    transport.onWrite = async (_sessionId, line) => {
      const outbound = JSON.parse(line) as { id: string | number; result: unknown };
      expect(outbound.id).toBe("permission-1");
      expect(outbound.result).toEqual({ outcome: "selected" });
    };
    client = new JsonRpcClient("inbound", {
      onRequest: (id, method, params) => {
        expect(id).toBe("permission-1");
        expect(method).toBe("session/request_permission");
        expect(params).toEqual({ options: ["allow"] });
        inbound.push(client.respond(id, { outcome: "selected" }));
      },
    });

    client.pushLine(JSON.stringify({
      jsonrpc: "2.0",
      id: "permission-1",
      method: "session/request_permission",
      params: { options: ["allow"] },
    }));
    await Promise.all(inbound);
  });

  it("rejects every pending request when the child closes", async () => {
    transport.onWrite = async () => undefined;
    const client = new JsonRpcClient("closed", {});
    const first = client.request("initialize");
    const second = client.request("session/new");

    client.close(new Error("child exited"));

    await expect(first).rejects.toThrow("child exited");
    await expect(second).rejects.toThrow("child exited");
    await expect(client.request("session/prompt")).rejects.toThrow(
      "Harness process is not running",
    );
  });

  it("does not resolve a request after its deadline", async () => {
    vi.useFakeTimers();
    try {
      let client!: JsonRpcClient;
      let requestId = 0;
      transport.onWrite = async (_sessionId, line) => {
        requestId = (JSON.parse(line) as { id: number }).id;
      };
      client = new JsonRpcClient("deadline", {});
      const request = client.request("initialize", undefined, 100);
      const settled = request.catch((error: Error) => error);
      await vi.advanceTimersByTimeAsync(101);
      await expect(settled).resolves.toMatchObject({ message: "initialize timed out" });

      client.pushLine(JSON.stringify({ jsonrpc: "2.0", id: requestId, result: {} }));
      expect(client.isClosed).toBe(false);
    } finally {
      vi.useRealTimers();
    }
  });
});
