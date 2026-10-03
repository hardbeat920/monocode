import { describe, expect, it, vi } from "vitest";
const sent = vi.hoisted(() => [] as string[]);
vi.mock("./child", () => ({
  writeChild: async (_id: string, line: string) => {
    sent.push(line);
  },
}));
import { AcpClient } from "./acp";

describe("ACP request identifiers", () => {
  it("handles requests before subsequent notifications", () => {
    const events: string[] = [];
    const client = new AcpClient("ordered", {
      onRequest: () => {
        events.push("request");
      },
      onNotification: () => {
        events.push("notification");
      },
    });
    client.pushLine(
      JSON.stringify({ jsonrpc: "2.0", id: 1, method: "cursor/task" }),
    );
    client.pushLine(
      JSON.stringify({ jsonrpc: "2.0", method: "session/update" }),
    );
    expect(events).toEqual(["request", "notification"]);
    client.close();
  });

  it.each(["sync", "async"])(
    "contains %s request handler failures",
    async (kind) => {
      const client = new AcpClient("failure", {
        onRequest: () => {
          if (kind === "sync") throw new Error("reply failed");
          return Promise.reject(new Error("reply failed"));
        },
      });
      expect(() =>
        client.pushLine(
          JSON.stringify({
            jsonrpc: "2.0",
            id: 1,
            method: "session/request_permission",
          }),
        ),
      ).not.toThrow();
      await new Promise((resolve) => setTimeout(resolve, 0));
      client.close();
    },
  );

  it("preserves string and number types when internal approval ids collide", async () => {
    const requests: number[] = [];
    const client = new AcpClient("ids", {
      onRequest: (id) => {
        requests.push(id);
      },
    });
    const incoming = [
      "permission-abc",
      Number.MAX_SAFE_INTEGER,
      "12",
      12,
      -1,
      1.5,
    ];
    for (const id of incoming)
      client.pushLine(
        JSON.stringify({
          jsonrpc: "2.0",
          id,
          method: "session/request_permission",
          params: {},
        }),
      );
    await Promise.resolve();
    expect(new Set(requests).size).toBe(incoming.length);
    expect(requests.every((id) => Number.isSafeInteger(id) && id >= 0)).toBe(
      true,
    );
    for (const id of requests)
      await client.respond(id, { outcome: { outcome: "cancelled" } });
    expect(sent.map((line) => JSON.parse(line).id)).toEqual(incoming);
    client.close();
  });
});
