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

/** ACP JSON-RPC client — thin wrapper preserving the Cursor numeric-id API. */
export class AcpClient {
  private readonly rpc: JsonRpcClient;

  constructor(
    sessionId: string,
    private readonly handlers: AcpHandlers,
  ) {
    const rpcHandlers: JsonRpcHandlers = {
      onNotification: (method, params) =>
        this.handlers.onNotification?.(method, params),
      onRequest: (id, method, params) => {
        const numeric =
          typeof id === "number" ? id : Number(id);
        void this.handlers.onRequest?.(numeric, method, params);
      },
    };
    this.rpc = new JsonRpcClient(sessionId, rpcHandlers, {
      includeJsonrpc: true,
      label: "acp",
    });
  }

  pushLine(line: string) {
    this.rpc.pushLine(line);
  }

  close(error?: Error) {
    this.rpc.close(error);
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
    return this.rpc.respond(id as JsonRpcId, result);
  }

  respondError(
    id: number,
    error: { code: number; message: string; data?: unknown },
  ): Promise<void> {
    return this.rpc.respondError(id as JsonRpcId, error);
  }
}
