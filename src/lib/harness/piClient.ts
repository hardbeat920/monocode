import { writeChild } from "./child";
import {
  asRecord,
  parseJsonLine,
  parseRpcResponse,
  stringField,
} from "./piProtocol";

type Pending = {
  resolve: (value: Record<string, unknown>) => void;
  reject: (error: Error) => void;
};
/**
 * Bounds mirror omp `packages/coding-agent/src/modes/rpc/rpc-frame.ts`:
 * one JSONL line may not exceed 1MB (`MAX_RPC_FRAME_BYTES`, newline included),
 * a reassembled logical frame may not exceed 64MB
 * (`MAX_RPC_REASSEMBLED_BYTES`), and chunk payloads are 256KB slices
 * (`RPC_CHUNK_PAYLOAD_BYTES`) sent as `rpc_chunk` frames with ids like `rpc-1`.
 */
const MAX_RPC_FRAME_BYTES = 1024 * 1024;
const MAX_RPC_REASSEMBLED_BYTES = 64 * 1024 * 1024;
const RPC_CHUNK_PAYLOAD_BYTES = 256 * 1024;
/** ceil(64MB / 256KB): the server rejects any count above this. */
const MAX_RPC_CHUNK_COUNT = 256;
const MAX_CHUNK_ID_LENGTH = 128;

const BASE64_RE =
  /^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/;

function base64ToBytes(data: string): Uint8Array {
  const binary = atob(data);
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index++) {
    bytes[index] = binary.charCodeAt(index);
  }
  return bytes;
}

function bytesToBase64(bytes: Uint8Array): string {
  let binary = "";
  const STEP = 0x8000;
  for (let index = 0; index < bytes.length; index += STEP) {
    binary += String.fromCharCode(...bytes.subarray(index, index + STEP));
  }
  return btoa(binary);
}

/** Strict base64 with the same round-trip check the omp decoder applies. */
function decodeChunkData(data: unknown): Uint8Array {
  if (typeof data !== "string" || data.length === 0 || !BASE64_RE.test(data)) {
    throw new Error("invalid rpc chunk data");
  }
  const bytes = base64ToBytes(data);
  if (bytesToBase64(bytes) !== data) throw new Error("invalid rpc chunk data");
  return bytes;
}

function concatBytes(parts: Uint8Array[]): Uint8Array {
  let total = 0;
  for (const part of parts) total += part.byteLength;
  const out = new Uint8Array(total);
  let offset = 0;
  for (const part of parts) {
    out.set(part, offset);
    offset += part.byteLength;
  }
  return out;
}

type AssemblerOutcome =
  | { consumed: false; complete?: undefined }
  | { consumed: true; complete?: Record<string, unknown> };

/**
 * Reassembles omp protocol v2 `rpc_chunk` frames into the complete response
 * or event they were split from, enforcing the same ceilings as the omp
 * `RpcFrameDecoder` (chunkId ≤128 chars, count ∈ [2, 256],
 * byteLength ∈ [1MB, 64MB], strict order from index 0, payload ≤256KB,
 * reassembled bytes == byteLength).
 *
 * Deliberately more lenient than the server in two places: a non-chunk line
 * arriving mid-sequence drops the sequence and flows through as a normal
 * frame instead of failing, and any index-0 chunk opens a fresh sequence
 * (chunk ids are strictly increasing, so the old one is already lost).
 * Anything that cannot be attributed to a request is dropped silently; the
 * owning request's timeout is the backstop.
 */
export class RpcChunkAssembler {
  private pending: {
    chunkId: string;
    count: number;
    byteLength: number;
    nextIndex: number;
    parts: Uint8Array[];
    receivedBytes: number;
  } | null = null;

  reset(): void {
    this.pending = null;
  }

  push(rec: Record<string, unknown>): AssemblerOutcome {
    if (stringField(rec, "type") !== "rpc_chunk") {
      this.pending = null;
      return { consumed: false };
    }
    try {
      const complete = this.pushChunk(rec);
      return { consumed: true, ...(complete ? { complete } : {}) };
    } catch {
      this.pending = null;
      return { consumed: true };
    }
  }

  private pushChunk(
    rec: Record<string, unknown>,
  ): Record<string, unknown> | undefined {
    const chunkId = stringField(rec, "chunkId");
    const { index, count, byteLength } = rec;
    if (
      !chunkId ||
      chunkId.length > MAX_CHUNK_ID_LENGTH ||
      typeof index !== "number" ||
      typeof count !== "number" ||
      typeof byteLength !== "number" ||
      !Number.isSafeInteger(index) ||
      !Number.isSafeInteger(count) ||
      !Number.isSafeInteger(byteLength) ||
      index < 0 ||
      count < 2 ||
      count > MAX_RPC_CHUNK_COUNT ||
      index >= count ||
      byteLength < MAX_RPC_FRAME_BYTES ||
      byteLength > MAX_RPC_REASSEMBLED_BYTES
    ) {
      throw new Error("invalid rpc chunk metadata");
    }
    const bytes = decodeChunkData(rec.data);
    if (bytes.byteLength > RPC_CHUNK_PAYLOAD_BYTES) {
      throw new Error("rpc chunk payload exceeds the transport limit");
    }
    // Any index-0 chunk opens a new sequence: the old one is unrecoverable
    // once the server moved on (chunk ids are `rpc-N`, strictly increasing),
    // while clinging to it would doom the new one too.
    let pending = this.pending;
    if (!pending || index === 0) {
      if (index !== 0) throw new Error("rpc chunk sequence must start at index 0");
      pending = {
        chunkId,
        count,
        byteLength,
        nextIndex: 0,
        parts: [],
        receivedBytes: 0,
      };
      this.pending = pending;
    }
    if (
      pending.chunkId !== chunkId ||
      pending.count !== count ||
      pending.byteLength !== byteLength ||
      pending.nextIndex !== index
    ) {
      throw new Error("rpc chunk sequence mismatch");
    }
    pending.parts.push(bytes);
    pending.receivedBytes += bytes.byteLength;
    pending.nextIndex += 1;
    if (pending.receivedBytes > pending.byteLength) {
      throw new Error("rpc chunk sequence exceeds declared length");
    }
    if (pending.nextIndex < pending.count) return undefined;
    if (pending.receivedBytes !== pending.byteLength) {
      throw new Error("rpc chunk sequence length mismatch");
    }
    this.pending = null;
    const json = new TextDecoder("utf-8", { fatal: true }).decode(
      concatBytes(pending.parts),
    );
    const frame = parseJsonLine(json);
    if (!frame) throw new Error("rpc frame must be an object");
    return frame;
  }
}

/**
 * JSONL request/response multiplexer for `pi --mode rpc` and the identical
 * `omp --mode rpc`. Agent events and extension UI frames go to `onFrame`.
 * Speaks protocol v1 by default; `negotiate()` upgrades omp connections to
 * v2 chunked frames so responses over the 1MB transport limit still arrive.
 */
export class PiRpc {
  private nextId = 1;
  private readonly pending = new Map<string, Pending>();
  private closed = false;
  private readonly assembler = new RpcChunkAssembler();

  constructor(
    private readonly sessionId: string,
    private readonly onFrame: (rec: Record<string, unknown>) => void,
    private readonly label = "Pi",
  ) {}

  pushLine(line: string) {
    let rec = parseJsonLine(line);
    if (!rec) return;
    const chunk = this.assembler.push(rec);
    if (chunk.consumed && !chunk.complete) return;
    if (chunk.complete) rec = chunk.complete;
    const response = parseRpcResponse(rec);
    if (response?.id && this.pending.has(response.id)) {
      const pending = this.pending.get(response.id);
      this.pending.delete(response.id);
      if (!pending) return;
      if (!response.success) {
        pending.reject(
          new Error(response.error || `${this.label} ${response.command} failed`),
        );
        return;
      }
      pending.resolve(rec);
      return;
    }
    this.onFrame(rec);
  }

  async request(
    command: Record<string, unknown>,
    timeoutMs = 15_000,
  ): Promise<Record<string, unknown>> {
    if (this.closed) throw new Error(`${this.label} process is not running`);
    const id = `mc_${this.nextId++}`;
    const payload = { ...command, id };
    const pending = new Promise<Record<string, unknown>>((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        const type = stringField(command, "type") ?? "command";
        reject(new Error(`${this.label} ${type} timed out`));
      }, timeoutMs);
      this.pending.set(id, {
        resolve: (value) => {
          clearTimeout(timer);
          resolve(value);
        },
        reject: (error) => {
          clearTimeout(timer);
          reject(error);
        },
      });
    });
    void writeChild(this.sessionId, JSON.stringify(payload)).catch((error) => {
      const message =
        error instanceof Error ? error : new Error(String(error));
      const request = this.pending.get(id);
      this.pending.delete(id);
      request?.reject(message);
    });
    return pending;
  }
  /**
   * Best-effort upgrade to omp protocol v2 chunked frames. The handshake
   * itself is a small v1 frame; only a `success:true` reply carrying
   * `data.protocolVersion === 2` enables reassembly. Anything else (unknown
   * command on Pi, timeout, closed transport) resolves false and the
   * connection stays on v1 with unchanged behavior.
   */
  async negotiate(timeoutMs = 5_000): Promise<boolean> {
    try {
      const rec = await this.request(
        { type: "negotiate_protocol", protocolVersion: 2 },
        timeoutMs,
      );
      return asRecord(rec.data)?.protocolVersion === 2;
    } catch {
      return false;
    }
  }

  close(error?: Error) {
    if (this.closed) return;
    this.closed = true;
    this.assembler.reset();
    const err = error ?? new Error(`${this.label} process exited`);
    for (const pending of this.pending.values()) pending.reject(err);
    this.pending.clear();
  }
}
