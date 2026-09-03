import { afterEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({ writeChild: vi.fn() }));

vi.mock("./child", () => ({ writeChild: mocks.writeChild }));

import { PiRpc, RpcChunkAssembler } from "./piClient";
import { asRecord } from "./piProtocol";

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
const CHUNK_BYTES = 256 * 1024;

function encodeChunks(
  payload: Record<string, unknown>,
  mutate?: (frame: Record<string, unknown>) => void,
): string[] {
  const bytes = new TextEncoder().encode(JSON.stringify(payload));
  const byteLength = bytes.byteLength;
  const count = Math.ceil(byteLength / CHUNK_BYTES);
  const lines: string[] = [];
  for (let index = 0; index < count; index++) {
    const slice = bytes.subarray(index * CHUNK_BYTES, (index + 1) * CHUNK_BYTES);
    let binary = "";
    for (const byte of slice) binary += String.fromCharCode(byte);
    const frame: Record<string, unknown> = {
      type: "rpc_chunk",
      chunkId: "rpc-1",
      index,
      count,
      byteLength,
      data: btoa(binary),
    };
    mutate?.(frame);
    lines.push(JSON.stringify(frame));
  }
  return lines;
}

function parseLine(line: string): Record<string, unknown> {
  const rec = asRecord(JSON.parse(line));
  if (!rec) throw new Error("test sent a non-object line");
  return rec;
}

function cmdId(cmd: Record<string, unknown>): string {
  if (typeof cmd.id !== "string") throw new Error("test command is missing id");
  return cmd.id;
}

/** Payload sized past the 1MB transport limit so chunking is legal. */
function bigPayload(id: string): Record<string, unknown> {
  return {
    id,
    type: "response",
    command: "get_available_models",
    success: true,
    data: {
      models: [{ id: "m1", provider: "p", name: "M1" }],
      pad: "x".repeat(1_200_000),
    },
  };
}

describe("PiRpc chunked frames", () => {
  it("reassembles a chunked large response into the pending request", async () => {
    const onFrame = vi.fn();
    const rpc = new PiRpc("probe", onFrame);
    mocks.writeChild.mockImplementation(
      async (_sessionId: string, line: string) => {
        const cmd = parseLine(line);
        for (const chunk of encodeChunks(bigPayload(cmdId(cmd)))) {
          rpc.pushLine(chunk);
        }
      },
    );

    const rec = await rpc.request({ type: "get_available_models" });
    const pad = asRecord(rec.data)?.pad;
    if (typeof pad !== "string") throw new Error("test response is missing pad");
    expect(pad).toHaveLength(1_200_000);
    expect(onFrame).not.toHaveBeenCalled();
    rpc.close();
  });
});

describe("RpcChunkAssembler", () => {
  function pushAll(
    assembler: RpcChunkAssembler,
    lines: string[],
  ): Record<string, unknown> | undefined {
    let complete: Record<string, unknown> | undefined;
    for (const line of lines) {
      const out = assembler.push(parseLine(line));
      expect(out.consumed).toBe(true);
      if (out.complete) complete = out.complete;
    }
    return complete;
  }

  it("rejects out-of-order chunks but heals on retry", () => {
    const assembler = new RpcChunkAssembler();
    const lines = encodeChunks(bigPayload("mc_1"));
    const shuffled = [lines[0], lines[2], lines[1], ...lines.slice(3)];
    expect(pushAll(assembler, shuffled)).toBeUndefined();
    expect(pushAll(assembler, lines)?.id).toBe("mc_1");
  });

  it("rejects out-of-range counts", () => {
    for (const count of [1, 300]) {
      const assembler = new RpcChunkAssembler();
      expect(
        pushAll(
          assembler,
          encodeChunks(bigPayload("mc_1"), (frame) => {
            frame.count = count;
          }),
        ),
      ).toBeUndefined();
    }
  });

  it("rejects out-of-range byte lengths", () => {
    for (const byteLength of [100, 100 * 1024 * 1024]) {
      const assembler = new RpcChunkAssembler();
      expect(
        pushAll(
          assembler,
          encodeChunks(bigPayload("mc_1"), (frame) => {
            frame.byteLength = byteLength;
          }),
        ),
      ).toBeUndefined();
    }
  });

  it("rejects invalid base64 and oversized payloads", () => {
    const badData = new RpcChunkAssembler();
    expect(
      pushAll(
        badData,
        encodeChunks(bigPayload("mc_1"), (frame) => {
          frame.data = "!!!not-base64!!!";
        }),
      ),
    ).toBeUndefined();

    const bigData = new Uint8Array(300 * 1024);
    let binary = "";
    for (const byte of bigData) binary += String.fromCharCode(byte);
    const oversized = new RpcChunkAssembler();
    expect(
      pushAll(
        oversized,
        encodeChunks(bigPayload("mc_1"), (frame) => {
          frame.count = 2;
          frame.byteLength = 2 * 1024 * 1024;
          frame.data = btoa(binary);
        }),
      ),
    ).toBeUndefined();
  });

  it("rejects sequences that do not start at index 0", () => {
    const assembler = new RpcChunkAssembler();
    expect(pushAll(assembler, encodeChunks(bigPayload("mc_1")).slice(1))).toBeUndefined();
  });

  it("rejects declared lengths that never add up", () => {
    const assembler = new RpcChunkAssembler();
    const payload = bigPayload("mc_1");
    const bytes = new TextEncoder().encode(JSON.stringify(payload)).byteLength;
    expect(
      pushAll(
        assembler,
        encodeChunks(payload, (frame) => {
          frame.byteLength = bytes + 1;
        }),
      ),
    ).toBeUndefined();
  });

  it("lets an interleaved normal frame through and drops the sequence", () => {
    const assembler = new RpcChunkAssembler();
    const lines = encodeChunks(bigPayload("mc_1"));
    expect(assembler.push(parseLine(lines[0])).consumed).toBe(true);
    const event = { type: "agent_end", messageCount: 0 };
    expect(assembler.push(event).consumed).toBe(false);
    expect(pushAll(assembler, lines)?.id).toBe("mc_1");
  });

  it("restarts when a new chunk id preempts the old one", () => {
    const assembler = new RpcChunkAssembler();
    const first = encodeChunks(bigPayload("mc_1"));
    const second = encodeChunks(bigPayload("mc_2"), (frame) => {
      frame.chunkId = "rpc-2";
    });
    expect(assembler.push(parseLine(first[0])).consumed).toBe(true);
    expect(pushAll(assembler, second)?.id).toBe("mc_2");
  });
});

describe("PiRpc.negotiate", () => {
  function serve(
    rpc: PiRpc,
    respond: (cmd: Record<string, unknown>) => string | string[] | undefined,
  ) {
    mocks.writeChild.mockImplementation(
      async (_sessionId: string, line: string) => {
        const out = respond(parseLine(line));
        for (const frame of out === undefined ? [] : [out].flat()) {
          rpc.pushLine(frame);
        }
      },
    );
  }

  it("returns true on a protocolVersion 2 reply", async () => {
    const rpc = new PiRpc("probe", vi.fn());
    serve(rpc, (cmd) =>
      JSON.stringify({
        id: cmd.id,
        type: "response",
        command: "negotiate_protocol",
        success: true,
        data: { protocolVersion: 2 },
      }),
    );
    await expect(rpc.negotiate()).resolves.toBe(true);
    rpc.close();
  });

  it("returns false on failure and keeps v1 working", async () => {
    const rpc = new PiRpc("probe", vi.fn());
    serve(rpc, (cmd) =>
      JSON.stringify({
        id: cmd.id,
        type: "response",
        command: "negotiate_protocol",
        success: false,
        error: "unknown command",
      }),
    );
    await expect(rpc.negotiate()).resolves.toBe(false);

    serve(rpc, (cmd) =>
      JSON.stringify({ id: cmd.id, type: "response", command: "ping", success: true }),
    );
    await expect(rpc.request({ type: "ping" })).resolves.toMatchObject({
      command: "ping",
    });
    rpc.close();
  });

  it("returns false on timeout", async () => {
    vi.useFakeTimers();
    mocks.writeChild.mockReturnValue(Promise.withResolvers<void>().promise);
    const rpc = new PiRpc("probe", vi.fn());
    const result = rpc.negotiate(100);
    await vi.advanceTimersByTimeAsync(100);
    await expect(result).resolves.toBe(false);
    rpc.close();
  });

  it("returns false when the transport write fails", async () => {
    mocks.writeChild.mockRejectedValue(new Error("write failed"));
    const rpc = new PiRpc("probe", vi.fn());
    await expect(rpc.negotiate()).resolves.toBe(false);
    rpc.close();
  });
});
