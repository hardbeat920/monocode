import { writeChild } from "../../core/child";
import { parseJsonLine, parseRpcResponse, stringField } from "./piProtocol";

type Pending = {
  resolve: (value: Record<string, unknown>) => void;
  reject: (error: Error) => void;
};

type ChunkRun = { id: string; count: number; parts: Uint8Array[] };

/**
 * JSONL request/response multiplexer for `pi --mode rpc` and the identical
 * `omp --mode rpc`. Agent events and extension UI frames go to `onFrame`.
 *
 * omp caps v1 responses at 1 MiB, which a large model catalog exceeds. When
 * its `ready` frame offers protocol v2, negotiate it: large frames then arrive
 * as base64 `rpc_chunk` parts that are reassembled here.
 */
export class PiRpc {
  private nextId = 1;
  private readonly pending = new Map<string, Pending>();
  private closed = false;
  private chunks: ChunkRun | undefined;
  private negotiated: Promise<void> | undefined;
  private onReady: (() => void) | undefined;
  private readonly readySeen = new Promise<void>((resolve) => {
    this.onReady = resolve;
  });

  constructor(
    private readonly sessionId: string,
    private readonly onFrame: (rec: Record<string, unknown>) => void,
    private readonly label = "Pi",
  ) {}

  pushLine(line: string) {
    const parsed = parseJsonLine(line);
    const rec = parsed?.type === "rpc_chunk" ? this.pushChunk(parsed) : parsed;
    if (!rec) return;
    if (rec.type === "ready") this.negotiate(rec);
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

  /** Resolves once `ready` and any v2 negotiation are done, or after `ms`. */
  waitReady(ms: number): Promise<void> {
    let timer: ReturnType<typeof setTimeout> | undefined;
    return Promise.race([
      this.readySeen.then(() => this.negotiated),
      new Promise<void>((resolve) => {
        timer = setTimeout(resolve, ms);
      }),
    ]).finally(() => clearTimeout(timer));
  }

  private negotiate(ready: Record<string, unknown>) {
    const versions = ready.supportedProtocolVersions;
    if (!this.negotiated && Array.isArray(versions) && versions.includes(2)) {
      this.negotiated = this.request({
        type: "negotiate_protocol",
        protocolVersion: 2,
      }).then(
        () => undefined,
        () => undefined,
      );
    }
    this.onReady?.();
  }

  /** Buffer one `rpc_chunk`; returns the reassembled frame on the last part. */
  private pushChunk(
    chunk: Record<string, unknown>,
  ): Record<string, unknown> | null {
    const { chunkId, index, count, data } = chunk;
    if (index === 0 && typeof chunkId === "string" && typeof count === "number")
      this.chunks = { id: chunkId, count, parts: [] };
    const run = this.chunks;
    if (
      !run ||
      run.id !== chunkId ||
      run.parts.length !== index ||
      typeof data !== "string"
    ) {
      this.chunks = undefined;
      return null;
    }
    run.parts.push(Uint8Array.from(atob(data), (c) => c.charCodeAt(0)));
    if (run.parts.length < run.count) return null;
    this.chunks = undefined;
    const bytes = new Uint8Array(
      run.parts.reduce((total, part) => total + part.length, 0),
    );
    let offset = 0;
    for (const part of run.parts) {
      bytes.set(part, offset);
      offset += part.length;
    }
    return parseJsonLine(new TextDecoder().decode(bytes));
  }

  async request(
    command: Record<string, unknown>,
    timeoutMs = 15_000,
  ): Promise<Record<string, unknown>> {
    if (this.negotiated && command.type !== "negotiate_protocol") {
      await this.negotiated;
    }
    if (this.closed) throw new Error(`${this.label} process is not running`);
    const id = stringField(command, "id") ?? `mc_${this.nextId++}`;
    if (this.pending.has(id)) throw new Error(`Duplicate RPC request id: ${id}`);
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

  close(error?: Error) {
    if (this.closed) return;
    this.closed = true;
    const err = error ?? new Error(`${this.label} process exited`);
    for (const pending of this.pending.values()) pending.reject(err);
    this.pending.clear();
  }

  cancelRequest(id: string): void {
    const pending = this.pending.get(id);
    this.pending.delete(id);
    pending?.reject(new Error(`${this.label} request cancelled`));
  }
}
