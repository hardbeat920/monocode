import { afterEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({ writeChild: vi.fn() }));

vi.mock("../../core/child", () => ({ writeChild: mocks.writeChild }));

import { PiRpc } from "./piClient";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((res) => {
    resolve = res;
  });
  return { promise, resolve };
}

afterEach(() => {
  mocks.writeChild.mockReset();
  vi.useRealTimers();
});

describe("PiRpc.request", () => {
  it("rejects when the transport write fails", async () => {
    mocks.writeChild.mockRejectedValue(new Error("write failed"));
    const rpc = new PiRpc("probe", vi.fn());

    await expect(rpc.request({ type: "get_commands" })).rejects.toThrow(
      "write failed",
    );
    rpc.close();
  });

  it("times out even when the transport write stalls", async () => {
    vi.useFakeTimers();
    const write = deferred<void>();
    mocks.writeChild.mockReturnValue(write.promise);
    const rpc = new PiRpc("probe", vi.fn());
    const request = rpc.request({ type: "get_commands" }, 100);
    const rejected = expect(request).rejects.toThrow(
      "Pi get_commands timed out",
    );

    await vi.advanceTimersByTimeAsync(100);
    await rejected;

    write.resolve();
    rpc.close();
  });
});

describe("PiRpc protocol v2", () => {
  it("negotiates v2 on ready and reassembles chunked responses", async () => {
    const written: Record<string, unknown>[] = [];
    mocks.writeChild.mockImplementation(async (_id: string, line: string) => {
      written.push(JSON.parse(line));
    });
    const rpc = new PiRpc("probe", vi.fn());
    rpc.pushLine(
      JSON.stringify({ type: "ready", supportedProtocolVersions: [1, 2] }),
    );
    await vi.waitFor(() => expect(written).toHaveLength(1));
    expect(written[0]).toMatchObject({
      type: "negotiate_protocol",
      protocolVersion: 2,
    });

    const request = rpc.request({ type: "get_available_models" });
    rpc.pushLine(
      JSON.stringify({
        type: "response",
        id: written[0].id,
        command: "negotiate_protocol",
        success: true,
        data: { protocolVersion: 2 },
      }),
    );
    await vi.waitFor(() => expect(written).toHaveLength(2));

    // Split mid-character so reassembly must join bytes, not strings.
    const bytes = new TextEncoder().encode(
      JSON.stringify({
        type: "response",
        id: written[1].id,
        command: "get_available_models",
        success: true,
        data: { models: [{ provider: "x", id: "é" }] },
      }),
    );
    const cut = bytes.indexOf(0xc3) + 1;
    const parts = [bytes.subarray(0, cut), bytes.subarray(cut)];
    parts.forEach((part, index) =>
      rpc.pushLine(
        JSON.stringify({
          type: "rpc_chunk",
          chunkId: "rpc-1",
          index,
          count: parts.length,
          data: btoa(String.fromCharCode(...part)),
        }),
      ),
    );

    await expect(request).resolves.toMatchObject({
      data: { models: [{ provider: "x", id: "é" }] },
    });
    rpc.close();
  });
});
