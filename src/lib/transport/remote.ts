import {
  canUseNativeSocket,
  createNativeSocket,
} from "./native";
import {
  buildPairWsUrl,
  COMPANION_PROTO_VERSION,
  decodeBytesEnvelope,
  isBytesEnvelope,
  isPairCode,
  isRpcResult,
  normalizePairCode,
  type PairClaimPayload,
  type RpcIncoming,
  type RpcRequest,
} from "./protocol";
import type {
  Transport,
  TransportEventHandler,
  UnlistenFn,
} from "./types";

/** Minimal WebSocket surface used. Injectable so tests can pass a fake. */
export type WebSocketLike = {
  send(data: string): void;
  close(code?: number, reason?: string): void;
  onopen: ((ev: unknown) => void) | null;
  onmessage: ((ev: { data: unknown }) => void) | null;
  onclose: ((ev: { code: number; reason: string }) => void) | null;
  onerror: ((ev: unknown) => void) | null;
};

export type WebSocketFactory = (url: string) => WebSocketLike;

export type RemoteStatus =
  | "connecting"
  | "open"
  | "closed";

export type RemoteOptions = {
  /** Per-request timeout. Host `harness_http` can take ~30s; default 60s. */
  invokeTimeoutMs?: number;
  /** Give up on one host and try the next. Default 8s. */
  connectTimeoutMs?: number;
  /** Reconnect with backoff after unexpected closes. Default true. */
  reconnect?: boolean;
  maxBackoffMs?: number;
  socket?: WebSocketFactory;
};

const DEFAULT_INVOKE_TIMEOUT_MS = 60_000;
const DEFAULT_MAX_BACKOFF_MS = 10_000;
const DEFAULT_CONNECT_TIMEOUT_MS = 8_000;

export function isCompanionAuthFailure(text: string): boolean {
  return /401|unauthorized|bad pairing token|pairing token rejected/i.test(
    text,
  );
}

function defaultSocket(url: string): WebSocketLike {
  // Tauri's WKWebView is https://tauri.localhost; a JS WebSocket to
  // ws://LAN is mixed content and never dials. Native TCP bypasses that.
  if (canUseNativeSocket()) return createNativeSocket(url);
  const Impl = globalThis.WebSocket as unknown as
    | (new (url: string) => WebSocketLike)
    | undefined;
  if (!Impl) throw new Error("companion: WebSocket is not available");
  return new Impl(url);
}

export function createWebSocket(url: string): WebSocketLike {
  return defaultSocket(url);
}

export type ClaimOptions = {
  timeoutMs?: number;
  socket?: WebSocketFactory;
};

const DEFAULT_CLAIM_TIMEOUT_MS = 15_000;

/**
 * Exchange the host's 6-digit pairing code for the real token over a
 * tokenless pair-mode socket. Tries ws:// first, then wss:// on the same
 * host:port, so manual entry never asks about connection types.
 */
export async function claimPairingCode(
  details: { host: string; port: number },
  code: string,
  options: ClaimOptions = {},
): Promise<PairClaimPayload> {
  const digits = normalizePairCode(code);
  if (!isPairCode(digits)) {
    throw new Error("Enter the 6-digit code shown on the host.");
  }
  const host = details.host.trim();
  const port = details.port;
  if (!host) throw new Error("Enter the host first.");
  const attempts = [
    buildPairWsUrl({ host, port, secure: false }),
    buildPairWsUrl({ host, port, secure: true }),
  ];
  let lastError: unknown = null;
  for (const url of attempts) {
    try {
      return await claimOnce(url, digits, options);
    } catch (error) {
      lastError = error;
    }
  }
  throw lastError instanceof Error
    ? lastError
    : new Error("Could not reach the host — check host, port, and network.");
}

function claimOnce(
  url: string,
  digits: string,
  options: ClaimOptions,
): Promise<PairClaimPayload> {
  const create = options.socket ?? defaultSocket;
  const timeoutMs = options.timeoutMs ?? DEFAULT_CLAIM_TIMEOUT_MS;
  return new Promise<PairClaimPayload>((resolve, reject) => {
    let settled = false;
    const done = (fn: () => void) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      try {
        socket.close();
      } catch {
        // Already gone; the result below is what matters.
      }
      fn();
    };
    const timer = setTimeout(() => {
      done(() => reject(new Error("The host did not answer — check host and port.")));
    }, timeoutMs);
    let socket: WebSocketLike;
    try {
      socket = create(url);
    } catch (error) {
      clearTimeout(timer);
      reject(error instanceof Error ? error : new Error(String(error)));
      return;
    }
    socket.onopen = () => {
      socket.send(
        JSON.stringify({
          id: 1,
          type: "invoke",
          command: "pair_claim",
          args: { code: digits },
        } satisfies RpcRequest),
      );
    };
    socket.onmessage = (event) => {
      let message: RpcIncoming;
      try {
        message = JSON.parse(event.data as string) as RpcIncoming;
      } catch {
        return;
      }
      if (!isRpcResult(message)) return;
      if (message.ok) {
        const payload = message.payload as PairClaimPayload | null;
        const token = payload?.token;
        if (typeof token === "string" && token) {
          done(() =>
            resolve({
              token,
              ...(payload?.lanIp ? { lanIp: payload.lanIp } : {}),
              ...(payload?.tailnetHost
                ? { tailnetHost: payload.tailnetHost }
                : {}),
            }),
          );
        } else {
          done(() => reject(new Error("The host answered, but without a token.")));
        }
      } else {
        done(() => reject(new Error(message.error || "Pairing rejected.")));
      }
    };
    socket.onerror = () => {
      done(() => reject(new Error("Could not reach the host.")));
    };
    socket.onclose = () => {
      done(() => reject(new Error("Could not reach the host.")));
    };
  });
}

type Pending = {
  resolve: (value: unknown) => void;
  reject: (error: Error) => void;
  timer: ReturnType<typeof setTimeout>;
};

export type RemoteStatusHandler = (status: RemoteStatus) => void;

/**
 * Thin-client transport: forwards `invoke` over the companion WebSocket and
 * fans out host-broadcast `event` frames to local listeners. Keeps the exact
 * same call signatures as Tauri so UI code needs no companion-specific forks.
 */
export class RemoteTransport implements Transport {
  readonly mode = "remote" as const;

  private readonly urls: string[];
  private urlIndex = 0;
  private readonly invokeTimeoutMs: number;
  private readonly connectTimeoutMs: number;
  private readonly reconnectEnabled: boolean;
  private readonly maxBackoffMs: number;
  private readonly createSocket: WebSocketFactory;

  private socket: WebSocketLike | null = null;
  private seq = 0;
  private readonly pending = new Map<number, Pending>();
  private readonly listeners = new Map<string, Set<TransportEventHandler<never>>>();
  private readonly statusHandlers = new Set<RemoteStatusHandler>();
  private status: RemoteStatus = "connecting";
  private reconnectAttempt = 0;
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private connectTimer: ReturnType<typeof setTimeout> | null = null;
  private disposed = false;

  constructor(url: string | readonly string[], options: RemoteOptions = {}) {
    const urls = (Array.isArray(url) ? [...url] : [url]).filter(
      (item) => item.length > 0,
    );
    if (urls.length === 0) {
      throw new Error("companion: missing websocket url");
    }
    this.urls = urls;
    this.invokeTimeoutMs =
      options.invokeTimeoutMs ?? DEFAULT_INVOKE_TIMEOUT_MS;
    this.connectTimeoutMs =
      options.connectTimeoutMs ?? DEFAULT_CONNECT_TIMEOUT_MS;
    this.reconnectEnabled = options.reconnect ?? true;
    this.maxBackoffMs = options.maxBackoffMs ?? DEFAULT_MAX_BACKOFF_MS;
    this.createSocket = options.socket ?? defaultSocket;
  }

  getStatus(): RemoteStatus {
    return this.status;
  }

  onStatusChange(handler: RemoteStatusHandler): UnlistenFn {
    this.statusHandlers.add(handler);
    return () => {
      this.statusHandlers.delete(handler);
    };
  }

  /** Dial the host. Resolves on open, rejects on the first error/close. */
  connect(): Promise<void> {
    if (this.disposed) return Promise.reject(new Error("companion: disposed"));
    return this.connectAttempt(0).catch((error) => {
      if (
        !this.disposed &&
        this.reconnectEnabled &&
        !isCompanionAuthFailure(
          error instanceof Error ? error.message : String(error),
        )
      ) {
        this.scheduleReconnect();
      }
      throw error;
    });
  }

  private connectAttempt(urlIndex: number): Promise<void> {
    if (this.disposed) return Promise.reject(new Error("companion: disposed"));
    if (urlIndex >= this.urls.length) {
      return Promise.reject(new Error("companion: host refused the connection"));
    }
    this.urlIndex = urlIndex;
    return this.connectOnce().catch((error) => {
      if (this.disposed) throw error;
      const message = error instanceof Error ? error.message : String(error);
      if (isCompanionAuthFailure(message)) throw error;
      if (urlIndex + 1 < this.urls.length) {
        return this.connectAttempt(urlIndex + 1);
      }
      throw error;
    });
  }

  private connectOnce(): Promise<void> {
    return new Promise((resolve, reject) => {
      let settled = false;
      const done = (fn: () => void) => {
        if (settled) return;
        settled = true;
        fn();
      };
      const onOpen = () => {
        done(() => {
          this.reconnectAttempt = 0;
          this.setStatus("open");
          resolve();
        });
      };
      const onEarlyClose = (reason: string) => {
        const message = isCompanionAuthFailure(reason)
          ? "companion: pairing token rejected — scan the code on the Mac again"
          : reason === "connect-timeout"
            ? "companion: timed out connecting to host"
            : "companion: host refused the connection";
        done(() => reject(new Error(message)));
      };
      try {
        this.openSocket(onOpen, onEarlyClose);
      } catch (error) {
        done(() =>
          reject(error instanceof Error ? error : new Error(String(error))),
        );
      }
    });
  }

  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
    if (this.disposed) return Promise.reject(new Error("companion: disposed"));
    if (!this.socket || this.status !== "open") {
      return Promise.reject(new Error("companion: not connected to host"));
    }
    this.seq = (this.seq + 1) % 0x7fffffff;
    const id = this.seq === 0 ? (this.seq = 1) : this.seq;
    const request: RpcRequest = { id, type: "invoke", command, args };
    return new Promise<T>((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`companion: request timed out (${command})`));
      }, this.invokeTimeoutMs);
      this.pending.set(id, {
        resolve: (value) => resolve(value as T),
        reject,
        timer,
      });
      try {
        this.socket?.send(JSON.stringify(request));
      } catch (error) {
        this.pending.delete(id);
        clearTimeout(timer);
        reject(error instanceof Error ? error : new Error(String(error)));
      }
    });
  }

  async listen<T>(
    event: string,
    handler: TransportEventHandler<T>,
  ): Promise<UnlistenFn> {
    let set = this.listeners.get(event);
    if (!set) {
      set = new Set();
      this.listeners.set(event, set);
    }
    set.add(handler as TransportEventHandler<never>);
    return () => {
      const live = this.listeners.get(event);
      if (!live) return;
      live.delete(handler as TransportEventHandler<never>);
      if (live.size === 0) this.listeners.delete(event);
    };
  }

  disconnect(): void {
    this.clearReconnectTimer();
    this.clearConnectTimer();
    this.failPending(new Error("companion: disconnected"));
    const socket = this.socket;
    this.socket = null;
    try {
      socket?.close(1000, "client disconnect");
    } catch {
      // Socket already gone; pending requests already failed above.
    }
    this.setStatus("closed");
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.disconnect();
    this.listeners.clear();
    this.statusHandlers.clear();
  }

  private setStatus(next: RemoteStatus): void {
    if (this.status === next) return;
    this.status = next;
    for (const handler of [...this.statusHandlers]) {
      try {
        handler(next);
      } catch {
        // Status observers must never break the socket.
      }
    }
  }

  private openSocket(
    onOpen: () => void,
    onEarlyClose: (reason: string) => void,
  ): void {
    this.clearConnectTimer();
    const previous = this.socket;
    this.socket = null;
    try {
      previous?.close(1000, "replaced");
    } catch {
      // Previous socket already gone.
    }
    const url = this.urls[this.urlIndex] ?? this.urls[0];
    const socket = this.createSocket(url);
    this.socket = socket;
    this.setStatus("connecting");
    this.connectTimer = setTimeout(() => {
      this.connectTimer = null;
      try {
        socket.close(1000, "connect-timeout");
      } catch {
        // Already gone; onclose still settles the waiter.
      }
    }, this.connectTimeoutMs);
    socket.onopen = () => {
      this.clearConnectTimer();
      onOpen();
    };
    socket.onmessage = (ev) => this.handleMessage(ev.data);
    socket.onerror = () => {
      // Browsers also fire close after error; early-close rejection is
      // handled there to avoid double-settling.
    };
    socket.onclose = (ev) => {
      this.clearConnectTimer();
      const wasSocket = this.socket === socket;
      if (!wasSocket) return;
      this.socket = null;
      const wasOpen = this.status === "open";
      this.failPending(
        new Error(`companion: connection closed (${ev.code})`),
      );
      const reason = ev.reason ?? "";
      onEarlyClose(reason);
      if (this.disposed) {
        this.setStatus("closed");
        return;
      }
      if (isCompanionAuthFailure(reason)) {
        this.setStatus("closed");
        return;
      }
      if (wasOpen && this.reconnectEnabled) {
        this.advanceUrl();
        this.scheduleReconnect();
      }
    };
  }

  private advanceUrl(): void {
    if (this.urls.length > 1) {
      this.urlIndex = (this.urlIndex + 1) % this.urls.length;
    }
  }

  private scheduleReconnect(): void {
    if (this.disposed || this.reconnectTimer) return;
    const backoff = Math.min(
      500 * 2 ** Math.min(this.reconnectAttempt, 5),
      this.maxBackoffMs,
    );
    this.reconnectAttempt += 1;
    this.setStatus("connecting");
    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = null;
      if (this.disposed) return;
      try {
        this.openSocket(
          () => {
            this.reconnectAttempt = 0;
            this.setStatus("open");
          },
          (reason) => {
            if (isCompanionAuthFailure(reason)) {
              this.setStatus("closed");
              return;
            }
            this.advanceUrl();
            this.scheduleReconnect();
          },
        );
      } catch {
        this.advanceUrl();
        this.scheduleReconnect();
      }
    }, backoff);
  }

  private clearReconnectTimer(): void {
    if (this.reconnectTimer) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
  }

  private clearConnectTimer(): void {
    if (this.connectTimer) {
      clearTimeout(this.connectTimer);
      this.connectTimer = null;
    }
  }

  private failPending(error: Error): void {
    if (this.pending.size === 0) return;
    const live = [...this.pending.values()];
    this.pending.clear();
    for (const entry of live) {
      clearTimeout(entry.timer);
      entry.reject(error);
    }
  }

  private handleMessage(data: unknown): void {
    if (typeof data !== "string") return;
    let message: RpcIncoming;
    try {
      message = JSON.parse(data) as RpcIncoming;
    } catch {
      return;
    }
    if (!message || typeof message !== "object") return;
    if (isRpcResult(message)) {
      const entry = this.pending.get(message.id);
      if (!entry) return;
      this.pending.delete(message.id);
      clearTimeout(entry.timer);
      if (message.ok) entry.resolve(decodeTransportPayload(message.payload));
      else entry.reject(new Error(message.error || "companion: host error"));
      return;
    }
    if (message.type === "event") {
      const set = this.listeners.get(message.event);
      if (!set || set.size === 0) return;
      const envelope = { payload: message.payload };
      for (const handler of [...set]) {
        try {
          (handler as TransportEventHandler<unknown>)(envelope);
        } catch {
          // One bad listener must not break the fan-out or the socket.
        }
      }
    }
  }
}

export { COMPANION_PROTO_VERSION };

/**
 * Host byte-commands arrive as `{ __bytes }` envelopes over JSON; decode to
 * the ArrayBuffer the local Tauri path would have delivered.
 */
function decodeTransportPayload(payload: unknown): unknown {
  if (isBytesEnvelope(payload)) return decodeBytesEnvelope(payload);
  return payload;
}
