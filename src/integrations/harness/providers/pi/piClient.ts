import { writeChild } from "../../core/child";
import type { PiFlavor } from "./piFlavor";
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

type HeldRequest = { id: string; line: string };

type ChunkSequence = {
  chunkId: string;
  count: number;
  byteLength: number;
  chunks: Uint8Array[];
  received: number;
};

// Protocol v2 limits, mirrored from omp's `RpcFrameDecoder`.
const MAX_FRAME_BYTES = 1024 * 1024;
const MAX_REASSEMBLED_BYTES = 64 * 1024 * 1024;
const CHUNK_PAYLOAD_BYTES = 256 * 1024;
const BASE64 =
  /^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/;

function isInteger(value: unknown): value is number {
  return Number.isSafeInteger(value);
}

function decodeBase64(data: unknown): Uint8Array {
  if (typeof data !== "string" || !data || !BASE64.test(data))
    throw new Error("invalid rpc chunk data");
  const binary = atob(data);
  // Like omp, reject encodings whose unused trailing bits are not zero.
  if (btoa(binary) !== data) throw new Error("invalid rpc chunk data");
  return Uint8Array.from(binary, (char) => char.charCodeAt(0));
}

/**
 * Reassembles protocol v2 `rpc_chunk` lines into the frame they carry, with
 * the checks of omp's `RpcFrameDecoder`. Any malformed or interrupted
 * sequence throws: the stream cannot resynchronize after one.
 */
class RpcChunkDecoder {
  private sequence?: ChunkSequence;

  push(rec: Record<string, unknown>): Record<string, unknown> | undefined {
    if (rec.type !== "rpc_chunk") {
      if (this.sequence) throw new Error("rpc chunk sequence interrupted");
      return rec;
    }
    const { chunkId, index, count, byteLength } = rec;
    if (
      typeof chunkId !== "string" ||
      !chunkId ||
      chunkId.length > 128 ||
      !isInteger(index) ||
      !isInteger(count) ||
      !isInteger(byteLength) ||
      index < 0 ||
      count < 2 ||
      count > MAX_REASSEMBLED_BYTES / CHUNK_PAYLOAD_BYTES ||
      index >= count ||
      byteLength < MAX_FRAME_BYTES ||
      byteLength > MAX_REASSEMBLED_BYTES
    )
      throw new Error("invalid rpc chunk metadata");
    const bytes = decodeBase64(rec.data);
    if (bytes.byteLength > CHUNK_PAYLOAD_BYTES)
      throw new Error("rpc chunk payload exceeds the transport limit");

    if (!this.sequence) {
      if (index !== 0)
        throw new Error("rpc chunk sequence must start at index 0");
      this.sequence = { chunkId, count, byteLength, chunks: [], received: 0 };
    }
    const sequence = this.sequence;
    if (
      sequence.chunkId !== chunkId ||
      sequence.count !== count ||
      sequence.byteLength !== byteLength ||
      sequence.chunks.length !== index
    )
      throw new Error("rpc chunk sequence mismatch");
    sequence.chunks.push(bytes);
    sequence.received += bytes.byteLength;
    if (sequence.received > byteLength)
      throw new Error("rpc chunk sequence exceeds declared length");
    if (sequence.chunks.length < count) return undefined;
    if (sequence.received !== byteLength)
      throw new Error("rpc chunk sequence length mismatch");

    this.sequence = undefined;
    const joined = new Uint8Array(byteLength);
    let offset = 0;
    for (const chunk of sequence.chunks) {
      joined.set(chunk, offset);
      offset += chunk.byteLength;
    }
    const text = new TextDecoder("utf-8", { fatal: true }).decode(joined);
    const frame = asRecord(JSON.parse(text));
    if (!frame) throw new Error("rpc frame must be an object");
    return frame;
  }
}

/**
 * JSONL request/response multiplexer for `pi --mode rpc` and the identical
 * `omp --mode rpc`. Agent events and extension UI frames go to `onFrame`.
 *
 * omp starts on protocol v1, which refuses any reply over 1 MiB, and offers
 * protocol v2 chunking in its `ready` frame. omp answers commands in stdin
 * order, so requests are held until that negotiation settles, or until the
 * flavor's grace passes without `ready`.
 */
export class PiRpc {
  private nextId = 1;
  private readonly pending = new Map<string, Pending>();
  private closed = false;
  private readonly label: string;
  private readonly readyGraceMs: number;
  private awaitingReady: boolean;
  private holding: boolean;
  private readonly held: HeldRequest[] = [];
  private handshakeTimer?: ReturnType<typeof setTimeout>;
  private negotiateId?: string;
  private chunks?: RpcChunkDecoder;
  private transportError?: Error;

  constructor(
    private readonly sessionId: string,
    private readonly onFrame: (rec: Record<string, unknown>) => void,
    flavor: Pick<PiFlavor, "label" | "rpcReadyGraceMs"> = { label: "Pi" },
  ) {
    this.label = flavor.label;
    this.readyGraceMs = flavor.rpcReadyGraceMs ?? 0;
    this.awaitingReady = flavor.rpcReadyGraceMs !== undefined;
    this.holding = this.awaitingReady;
  }

  pushLine(line: string) {
    if (this.transportError) return;
    const parsed = parseJsonLine(line);
    if (!parsed) return;
    let rec: Record<string, unknown> | undefined;
    try {
      rec = this.chunks ? this.chunks.push(parsed) : parsed;
    } catch (error) {
      this.failTransport(error);
      return;
    }
    if (!rec) return;
    const response = parseRpcResponse(rec);
    if (response?.id && response.id === this.negotiateId) {
      this.negotiateId = undefined;
      // Decode by the answer even if it outlived the grace: omp starts
      // chunking right after a successful response.
      if (response.success && asRecord(response.data)?.protocolVersion === 2)
        this.chunks = new RpcChunkDecoder();
      this.release();
      return;
    }
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
    if (this.awaitingReady && rec.type === "ready") this.negotiate(rec);
  }

  async request(
    command: Record<string, unknown>,
    timeoutMs = 15_000,
  ): Promise<Record<string, unknown>> {
    if (this.closed)
      throw this.transportError ?? new Error(`${this.label} process is not running`);
    const id = stringField(command, "id") ?? `mc_${this.nextId++}`;
    if (this.pending.has(id)) throw new Error(`Duplicate RPC request id: ${id}`);
    const line = JSON.stringify({ ...command, id });
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
    if (this.holding) {
      this.held.push({ id, line });
      this.handshakeTimer ??= setTimeout(
        () => this.release(),
        this.readyGraceMs,
      );
    } else {
      this.write(id, line);
    }
    return pending;
  }

  close(error?: Error) {
    if (this.closed) return;
    this.closed = true;
    clearTimeout(this.handshakeTimer);
    this.held.length = 0;
    const err = error ?? new Error(`${this.label} process exited`);
    for (const pending of this.pending.values()) pending.reject(err);
    this.pending.clear();
  }

  cancelRequest(id: string): void {
    const pending = this.pending.get(id);
    this.pending.delete(id);
    pending?.reject(new Error(`${this.label} request cancelled`));
  }

  private write(id: string, line: string) {
    void writeChild(this.sessionId, line).catch((error) => {
      const message = error instanceof Error ? error : new Error(String(error));
      const request = this.pending.get(id);
      this.pending.delete(id);
      request?.reject(message);
    });
  }

  private negotiate(ready: Record<string, unknown>) {
    this.awaitingReady = false;
    const versions = ready.supportedProtocolVersions;
    if (!Array.isArray(versions) || !versions.includes(2)) {
      this.release();
      return;
    }
    const id = `mc_${this.nextId++}`;
    this.negotiateId = id;
    this.holding = true;
    clearTimeout(this.handshakeTimer);
    this.handshakeTimer = setTimeout(() => this.release(), this.readyGraceMs);
    const line = JSON.stringify({
      type: "negotiate_protocol",
      protocolVersion: 2,
      id,
    });
    void writeChild(this.sessionId, line).catch(() => this.release());
  }

  /** Settle the handshake on the current protocol and send held requests. */
  private release() {
    clearTimeout(this.handshakeTimer);
    this.handshakeTimer = undefined;
    this.holding = false;
    for (const { id, line } of this.held.splice(0)) {
      if (this.pending.has(id)) this.write(id, line);
    }
  }

  private failTransport(cause: unknown) {
    const reason = cause instanceof Error ? cause.message : String(cause);
    this.transportError = new Error(
      `${this.label} RPC transport failed: ${reason}`,
    );
    this.close(this.transportError);
  }
}
