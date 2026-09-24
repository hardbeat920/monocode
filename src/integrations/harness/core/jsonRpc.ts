import { writeChild } from "./child";

type Pending = {
  resolve: (value: unknown) => void;
  reject: (error: Error) => void;
};

export type JsonRpcId = number | string;

export type JsonRpcMessage = {
  jsonrpc?: string;
  id?: JsonRpcId;
  method?: string;
  params?: unknown;
  result?: unknown;
  error?: { code?: number; message?: string; data?: unknown };
};

// A child wedged hard enough to block stdin writes is unrecoverable; the
// write deadline fails the request so the caller can recycle the generation.
const WRITE_TIMEOUT_MS = 15_000;

export type JsonRpcHandlers = {
  onNotification?: (method: string, params: unknown) => void;
  onRequest?: (
    id: JsonRpcId,
    method: string,
    params: unknown,
  ) => void | Promise<void>;
};

export type JsonRpcClientOptions = {
  /** Include `"jsonrpc":"2.0"` on outbound messages. Default true (ACP). Codex omits it. */
  includeJsonrpc?: boolean;
  label?: string;
};

/**
 * Bidirectional JSON-RPC / JSONL client over a harness child process stdin/stdout.
 * Supports numeric and string request ids; Codex app-server uses headerless frames.
 */
export class JsonRpcClient {
  private nextId = 1;
  private readonly pending = new Map<string, Pending>();
  private closed = false;
  private readonly includeJsonrpc: boolean;
  private readonly label: string;

  constructor(
    private readonly sessionId: string,
    private readonly handlers: JsonRpcHandlers,
    options: JsonRpcClientOptions = {},
  ) {
    this.includeJsonrpc = options.includeJsonrpc !== false;
    this.label = options.label ?? "rpc";
  }

  pushLine(line: string) {
    const trimmed = line.trim();
    if (!trimmed) return;
    let msg: JsonRpcMessage;
    try {
      msg = JSON.parse(trimmed) as JsonRpcMessage;
    } catch {
      console.debug(
        `[${this.label} ${this.sessionId}] non-json`,
        trimmed.slice(0, 200),
      );
      return;
    }
    this.handle(msg);
  }

  close(error?: Error) {
    if (this.closed) return;
    this.closed = true;
    const err = error ?? new Error("Harness process exited");
    this.rejectPending(err);
  }

  /** Drop in-flight client requests so a cancelled turn can unwind. */
  rejectPending(error?: Error) {
    const err = error ?? new Error("cancelled");
    for (const [, pending] of this.pending) pending.reject(err);
    this.pending.clear();
  }

  get isClosed(): boolean {
    return this.closed;
  }

  private key(id: JsonRpcId): string {
    return `${typeof id}:${String(id)}`;
  }

  async request<T>(
    method: string,
    params?: unknown,
    timeoutMs = 0,
  ): Promise<T> {
    if (this.closed) throw new Error("Harness process is not running");
    const id = this.nextId++;
    const key = this.key(id);
    // Register before writing. A local harness can answer quickly enough for
    // Tauri to deliver its stdout event before harness_write resolves; adding
    // the pending entry after send() silently discarded that response.
    const response = new Promise<T>((resolve, reject) => {
      const timer =
        timeoutMs > 0
          ? setTimeout(() => {
              this.pending.delete(key);
              reject(new Error(`${method} timed out`));
            }, timeoutMs)
          : undefined;
      this.pending.set(key, {
        resolve: (value) => {
          if (timer) clearTimeout(timer);
          resolve(value as T);
        },
        reject: (error) => {
          if (timer) clearTimeout(timer);
          reject(error);
        },
      });
    });
    // The deadline may settle `response` while we are still suspended in
    // send() below; mark it handled so that window can't surface as an
    // unhandled rejection. The promise returned to the caller still settles.
    response.catch(() => undefined);
    try {
      await this.send({
        ...(this.includeJsonrpc ? { jsonrpc: "2.0" } : {}),
        id,
        method,
        ...(params !== undefined ? { params } : {}),
      });
    } catch (error) {
      const pending = this.pending.get(key);
      if (pending) {
        this.pending.delete(key);
        pending.reject(
          error instanceof Error ? error : new Error(String(error)),
        );
      }
    }
    return response;
  }

  async notify(method: string, params?: unknown): Promise<void> {
    if (this.closed) return;
    await this.send({
      ...(this.includeJsonrpc ? { jsonrpc: "2.0" } : {}),
      method,
      ...(params !== undefined ? { params } : {}),
    });
  }

  async respond(id: JsonRpcId, result: unknown): Promise<void> {
    await this.send({
      ...(this.includeJsonrpc ? { jsonrpc: "2.0" } : {}),
      id,
      result,
    });
  }

  async respondError(
    id: JsonRpcId,
    error: { code: number; message: string; data?: unknown },
  ): Promise<void> {
    await this.send({
      ...(this.includeJsonrpc ? { jsonrpc: "2.0" } : {}),
      id,
      error,
    });
  }

  private async send(payload: object): Promise<void> {
    // Bound the write: a child that stops draining stdin must not let a
    // blocked harness_write outlive the request's own deadline (or wedge a
    // cancellation waiting on the session/cancel notify).
    const line = JSON.stringify(payload);
    let timer: ReturnType<typeof setTimeout> | undefined;
    try {
      await Promise.race([
        writeChild(this.sessionId, line),
        new Promise<never>((_, reject) => {
          timer = setTimeout(
            () => reject(new Error("harness write timed out")),
            WRITE_TIMEOUT_MS,
          );
        }),
      ]);
    } finally {
      if (timer) clearTimeout(timer);
    }
  }

  private handle(msg: JsonRpcMessage) {
    if (msg.id != null && (msg.result !== undefined || msg.error)) {
      const key = this.key(msg.id);
      const pending = this.pending.get(key);
      if (!pending) return;
      this.pending.delete(key);
      if (msg.error) {
        pending.reject(
          new Error(
            msg.error.message || `${this.label} error ${msg.error.code ?? ""}`,
          ),
        );
        return;
      }
      pending.resolve(msg.result);
      return;
    }

    if (msg.method && msg.id != null) {
      void this.handlers.onRequest?.(msg.id, msg.method, msg.params);
      return;
    }

    if (msg.method) {
      this.handlers.onNotification?.(msg.method, msg.params);
    }
  }
}
