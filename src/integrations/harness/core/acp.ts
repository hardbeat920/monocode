import {
  JsonRpcClient,
  type JsonRpcHandlers,
  type JsonRpcId,
  type JsonRpcMessage,
} from "./jsonRpc";

export type { JsonRpcMessage };

export type AcpHandlers = {
  onNotification?: (method: string, params: unknown) => void;
  onRequest?: (
    id: number,
    method: string,
    params: unknown,
  ) => void | Promise<void>;
};

/** Numeric UI approval ids map back to the original JSON-RPC request ids. */
export class AcpClient {
  private readonly rpc: JsonRpcClient;
  private readonly requestIds = new Map<number, JsonRpcId>();
  // The host accepts non-negative safe integers for UI approval ids.
  private nextRequestId = Number.MAX_SAFE_INTEGER;

  constructor(
    sessionId: string,
    private readonly handlers: AcpHandlers,
  ) {
    const rpcHandlers: JsonRpcHandlers = {
      onNotification: (method, params) =>
        this.handlers.onNotification?.(method, params),
      onRequest: (id, method, params) => {
        let numeric =
          typeof id === "number" && Number.isSafeInteger(id) && id >= 0
            ? id
            : this.nextRequestId--;
        while (this.requestIds.has(numeric)) numeric = this.nextRequestId--;
        this.requestIds.set(numeric, id);
        try {
          void Promise.resolve(
            this.handlers.onRequest?.(numeric, method, params),
          ).catch(() => undefined);
        } catch {
          return;
        }
      },
    };
    this.rpc = new JsonRpcClient(sessionId, rpcHandlers, {
      includeJsonrpc: true,
      label: "acp",
    });
  }

  private rawId(id: number): JsonRpcId {
    const raw = this.requestIds.get(id) ?? id;
    this.requestIds.delete(id);
    return raw;
  }

  pushLine(line: string) {
    this.rpc.pushLine(line);
  }

  close(error?: Error) {
    this.rpc.close(error);
    this.requestIds.clear();
  }

  rejectPending(error?: Error) {
    this.rpc.rejectPending(error);
  }

  request<T>(method: string, params?: unknown, timeoutMs = 0): Promise<T> {
    return this.rpc.request<T>(method, params, timeoutMs);
  }

  notify(method: string, params?: unknown): Promise<void> {
    return this.rpc.notify(method, params);
  }

  respond(id: number, result: unknown): Promise<void> {
    return this.rpc.respond(this.rawId(id), result);
  }

  respondError(
    id: number,
    error: { code: number; message: string; data?: unknown },
  ): Promise<void> {
    return this.rpc.respondError(this.rawId(id), error);
  }
}
