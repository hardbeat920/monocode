import { describe, expect, it, vi } from "vitest";

const sent: string[] = [];

vi.mock("./child", () => ({
  writeChild: async (_sessionId: string, line: string) => {
    sent.push(line);
  },
}));

import { AcpClient } from "./acp";

describe("AcpClient", () => {
  it("preserves string request IDs through the raw handler", async () => {
    sent.length = 0;
    let client!: AcpClient;
    const request = new Promise<void>((resolve) => {
      client = new AcpClient("string-id", {
        onRequestRaw: (id, method, params) => {
          expect(id).toBe("permission-1");
          expect(method).toBe("session/request_permission");
          expect(params).toEqual({ options: ["allow"] });
          void client.respond(id, { outcome: "selected" }).then(resolve);
        },
      });
    });

    client.pushLine(JSON.stringify({
      jsonrpc: "2.0",
      id: "permission-1",
      method: "session/request_permission",
      params: { options: ["allow"] },
    }));
    await request;
    expect(JSON.parse(sent[0])).toMatchObject({
      jsonrpc: "2.0",
      id: "permission-1",
      result: { outcome: "selected" },
    });
  });

  it("keeps numeric request IDs on the legacy handler", async () => {
    let received: number | undefined;
    const client = new AcpClient("numeric-id", {
      onRequest: (id) => {
        received = id;
      },
    });
    client.pushLine(JSON.stringify({
      jsonrpc: "2.0",
      id: 7,
      method: "session/request_permission",
    }));
    expect(received).toBe(7);
  });

  it("preserves string IDs for protocol errors", async () => {
    sent.length = 0;
    const client = new AcpClient("string-error", {});
    await client.respondError("permission-2", {
      code: -32601,
      message: "Method not found",
    });
    expect(JSON.parse(sent[0])).toMatchObject({
      id: "permission-2",
      error: { code: -32601, message: "Method not found" },
    });
  });
});
