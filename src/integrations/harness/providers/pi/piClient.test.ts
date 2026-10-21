import { afterEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({ writeChild: vi.fn() }));

vi.mock("../../core/child", () => ({
  resolveOmpBinary: vi.fn(),
  resolvePiBinary: vi.fn(),
  writeChild: mocks.writeChild,
}));

import { PiRpc } from "./piClient";
import { OMP_FLAVOR, PI_FLAVOR } from "./piFlavor";

const READY = {
  type: "ready",
  protocolVersion: 1,
  supportedProtocolVersions: [1, 2],
  maxFrameBytes: 1024 * 1024,
  maxReassembledFrameBytes: 64 * 1024 * 1024,
};
const CHUNK_BYTES = 256 * 1024;

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((res) => {
    resolve = res;
  });
  return { promise, resolve };
}

function written(): Record<string, unknown>[] {
  return mocks.writeChild.mock.calls.map(([, line]) => JSON.parse(line));
}

function push(rpc: PiRpc, frame: unknown) {
  rpc.pushLine(JSON.stringify(frame));
}

/** Split a frame the way omp 18.11 does once protocol v2 is negotiated. */
function chunkFrames(frame: object, chunkId = "chunk-1") {
  const bytes = Buffer.from(JSON.stringify(frame));
  const count = Math.ceil(bytes.byteLength / CHUNK_BYTES);
  return Array.from({ length: count }, (_, index) => ({
    type: "rpc_chunk",
    chunkId,
    index,
    count,
    byteLength: bytes.byteLength,
    data: bytes
      .subarray(index * CHUNK_BYTES, (index + 1) * CHUNK_BYTES)
      .toString("base64"),
  }));
}

/** A response comfortably over omp's 1 MiB line limit. */
function largeResponse(id: unknown) {
  return {
    type: "response",
    id,
    command: "get_state",
    success: true,
    data: { padding: "x".repeat(1_300_000) },
  };
}

const NEGOTIATED = { success: true, data: { protocolVersion: 2 } };

function negotiateResponse(result: Record<string, unknown>) {
  const negotiate = written().find(
    (command) => command.type === "negotiate_protocol",
  );
  if (!negotiate) throw new Error("negotiate_protocol was not written");
  return {
    type: "response",
    id: negotiate.id,
    command: "negotiate_protocol",
    ...result,
  };
}

function connectV2(onFrame = vi.fn()) {
  const rpc = new PiRpc("probe", onFrame, OMP_FLAVOR);
  push(rpc, READY);
  push(rpc, negotiateResponse(NEGOTIATED));
  return rpc;
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

describe("PiRpc protocol negotiation", () => {
  it("holds requests issued before ready until the negotiation response", async () => {
    mocks.writeChild.mockResolvedValue(undefined);
    const onFrame = vi.fn();
    const rpc = new PiRpc("probe", onFrame, OMP_FLAVOR);
    const request = rpc.request({ type: "get_state" });
    expect(mocks.writeChild).not.toHaveBeenCalled();

    push(rpc, READY);
    expect(onFrame).toHaveBeenCalledExactlyOnceWith(READY);
    expect(written()).toEqual([
      {
        type: "negotiate_protocol",
        protocolVersion: 2,
        id: expect.any(String),
      },
    ]);

    push(rpc, negotiateResponse(NEGOTIATED));
    const [, held] = written();
    expect(held).toEqual({ type: "get_state", id: expect.any(String) });
    expect(onFrame).toHaveBeenCalledTimes(1);

    for (const chunk of chunkFrames(largeResponse(held.id))) push(rpc, chunk);
    await expect(request).resolves.toEqual(largeResponse(held.id));
    rpc.close();
  });

  it("writes a Pi request immediately and leaves rpc_chunk lines to onFrame", async () => {
    mocks.writeChild.mockResolvedValue(undefined);
    const onFrame = vi.fn();
    const rpc = new PiRpc("probe", onFrame, PI_FLAVOR);
    const request = rpc.request({ type: "get_state" }, 100);
    expect(mocks.writeChild).toHaveBeenCalledExactlyOnceWith(
      "probe",
      '{"type":"get_state","id":"mc_1"}',
    );

    const [chunk] = chunkFrames(largeResponse("mc_1"));
    push(rpc, chunk);
    expect(onFrame).toHaveBeenCalledExactlyOnceWith(chunk);
    await expect(request).rejects.toThrow("Pi get_state timed out");
    rpc.close();
  });

  it.each([
    [
      "a failed response",
      { success: false, error: "Unsupported RPC protocol version: 2" },
    ],
    [
      "an unexpected protocol version",
      { success: true, data: { protocolVersion: 1 } },
    ],
  ])("stays on protocol v1 after %s", async (_, response) => {
    mocks.writeChild.mockResolvedValue(undefined);
    const onFrame = vi.fn();
    const rpc = new PiRpc("probe", onFrame, OMP_FLAVOR);
    const request = rpc.request({ type: "get_state" });
    push(rpc, READY);
    push(rpc, negotiateResponse(response));

    const [, held] = written();
    expect(held).toEqual({ type: "get_state", id: expect.any(String) });
    const [chunk] = chunkFrames(largeResponse(held.id));
    push(rpc, chunk);
    expect(onFrame).toHaveBeenLastCalledWith(chunk);
    push(rpc, {
      type: "response",
      id: held.id,
      command: "get_state",
      success: true,
    });
    await expect(request).resolves.toMatchObject({ id: held.id });
    rpc.close();
  });

  it("sends held requests on protocol v1 when ready never arrives", async () => {
    vi.useFakeTimers();
    mocks.writeChild.mockResolvedValue(undefined);
    const onFrame = vi.fn();
    const rpc = new PiRpc("probe", onFrame, OMP_FLAVOR);
    const request = rpc.request({ type: "get_state" });

    await vi.advanceTimersByTimeAsync(4_999);
    expect(mocks.writeChild).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);
    expect(written()).toEqual([{ type: "get_state", id: "mc_1" }]);

    const [chunk] = chunkFrames(largeResponse("mc_1"));
    push(rpc, chunk);
    expect(onFrame).toHaveBeenCalledExactlyOnceWith(chunk);
    push(rpc, {
      type: "response",
      id: "mc_1",
      command: "get_state",
      success: true,
    });
    await expect(request).resolves.toMatchObject({ id: "mc_1" });
    rpc.close();
    expect(vi.getTimerCount()).toBe(0);
  });

  it("sends held requests on protocol v1 when negotiation never answers", async () => {
    vi.useFakeTimers();
    mocks.writeChild.mockResolvedValue(undefined);
    const rpc = new PiRpc("probe", vi.fn(), OMP_FLAVOR);
    const request = rpc.request({ type: "get_state" });
    await vi.advanceTimersByTimeAsync(4_000);
    push(rpc, READY);

    await vi.advanceTimersByTimeAsync(4_999);
    expect(written().map((command) => command.type)).toEqual([
      "negotiate_protocol",
    ]);
    await vi.advanceTimersByTimeAsync(1);
    const [, held] = written();
    expect(held).toEqual({ type: "get_state", id: expect.any(String) });
    push(rpc, {
      type: "response",
      id: held.id,
      command: "get_state",
      success: true,
    });
    await expect(request).resolves.toMatchObject({ id: held.id });
    rpc.close();
  });

  it("times out a held request without writing it later", async () => {
    vi.useFakeTimers();
    mocks.writeChild.mockResolvedValue(undefined);
    const rpc = new PiRpc("probe", vi.fn(), OMP_FLAVOR);
    const rejected = expect(
      rpc.request({ type: "get_state" }, 1_000),
    ).rejects.toThrow("omp get_state timed out");

    await vi.advanceTimersByTimeAsync(1_000);
    await rejected;
    await vi.advanceTimersByTimeAsync(4_000);
    expect(mocks.writeChild).not.toHaveBeenCalled();
    rpc.close();
  });
});

describe("PiRpc protocol v2 chunks", () => {
  function frames(id: unknown) {
    return chunkFrames(largeResponse(id));
  }

  it.each([
    [
      "bad base64",
      (id: unknown) => {
        const [first, ...rest] = frames(id);
        return [{ ...first, data: `${first.data.slice(0, -4)}!!!!` }, ...rest];
      },
      "invalid rpc chunk data",
    ],
    [
      "non-canonical base64",
      (id: unknown) => {
        const [first, ...rest] = frames(id);
        // "A" encodes as "QQ=="; "QR==" decodes to it with non-zero spare bits.
        return [{ ...first, data: `${first.data.slice(0, -4)}QR==` }, ...rest];
      },
      "invalid rpc chunk data",
    ],
    [
      "an out-of-order index",
      (id: unknown) => {
        const [first, second, ...rest] = frames(id);
        return [first, ...rest, second];
      },
      "rpc chunk sequence mismatch",
    ],
    [
      "a sequence that does not start at index 0",
      (id: unknown) => frames(id).slice(1),
      "rpc chunk sequence must start at index 0",
    ],
    [
      "an interrupted sequence",
      (id: unknown) => {
        const [first, ...rest] = frames(id);
        return [first, { type: "agent_start" }, ...rest];
      },
      "rpc chunk sequence interrupted",
    ],
    [
      "a length mismatch",
      (id: unknown) =>
        frames(id).map((chunk) => ({
          ...chunk,
          byteLength: chunk.byteLength + 1,
        })),
      "rpc chunk sequence length mismatch",
    ],
    [
      "invalid metadata",
      (id: unknown) => frames(id).map((chunk) => ({ ...chunk, count: 1 })),
      "invalid rpc chunk metadata",
    ],
  ])("fails the connection on %s", async (_, corrupt, reason) => {
    mocks.writeChild.mockResolvedValue(undefined);
    const onFrame = vi.fn();
    const rpc = connectV2(onFrame);
    const first = rpc.request({ type: "get_state" });
    const second = rpc.request({ type: "get_commands" });
    const [, held] = written();

    for (const line of corrupt(held.id)) push(rpc, line);
    const error = `omp RPC transport failed: ${reason}`;
    await expect(first).rejects.toThrow(error);
    await expect(second).rejects.toThrow(error);
    await expect(rpc.request({ type: "get_state" })).rejects.toThrow(error);
    push(rpc, { type: "agent_end" });
    expect(onFrame).toHaveBeenCalledExactlyOnceWith(READY);
  });

  it("delivers a reassembled event frame to onFrame", () => {
    mocks.writeChild.mockResolvedValue(undefined);
    const onFrame = vi.fn();
    const rpc = connectV2(onFrame);
    const event = { type: "message_end", text: "y".repeat(1_100_000) };

    const chunks = chunkFrames(event);
    const last = chunks.pop()!;
    for (const chunk of chunks) push(rpc, chunk);
    expect(onFrame).toHaveBeenCalledTimes(1);
    push(rpc, last);
    expect(onFrame).toHaveBeenLastCalledWith(event);
    rpc.close();
  });
});
