import { isTauriRuntime } from "./isTauri";

type PendingInvoke = {
  resolve: (value: unknown) => void;
  reject: (error: Error) => void;
};

type ServerMessage = {
  type: string;
  id?: number;
  ok?: boolean;
  result?: unknown;
  error?: string;
  name?: string;
  payload?: unknown;
};

type EventHandler = (payload: unknown) => void;

let socket: WebSocket | null = null;
let nextId = 1;
const pending = new Map<number, PendingInvoke>();
const listeners = new Map<string, Set<EventHandler>>();
let connectPromise: Promise<void> | null = null;
let storedToken: string | null = null;

function wsUrl(token: string): string {
  const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
  return `${protocol}//${window.location.host}/api/remote/ws?token=${encodeURIComponent(token)}`;
}

function dispatchEvent(name: string, payload: unknown) {
  const handlers = listeners.get(name);
  if (!handlers) return;
  for (const handler of handlers) {
    handler(payload);
  }
}

function handleMessage(raw: string) {
  let message: ServerMessage;
  try {
    message = JSON.parse(raw) as ServerMessage;
  } catch {
    return;
  }

  if (message.type === "event" && message.name) {
    dispatchEvent(message.name, message.payload);
    return;
  }

  if (message.type === "invoke" && message.id != null) {
    const entry = pending.get(message.id);
    if (!entry) return;
    pending.delete(message.id);
    if (message.ok) {
      entry.resolve(message.result);
    } else {
      entry.reject(new Error(message.error ?? "Remote invoke failed"));
    }
  }
}

function ensureSocket(token: string): Promise<void> {
  if (socket && socket.readyState === WebSocket.OPEN && storedToken === token) {
    return Promise.resolve();
  }

  if (connectPromise && storedToken === token) {
    return connectPromise;
  }

  storedToken = token;
  if (socket) {
    socket.close();
    socket = null;
  }

  connectPromise = new Promise((resolve, reject) => {
    const ws = new WebSocket(wsUrl(token));
    socket = ws;

    ws.onopen = () => {
      connectPromise = null;
      resolve();
    };

    ws.onerror = () => {
      connectPromise = null;
      reject(new Error("Failed to connect to MonoCode remote server"));
    };

    ws.onclose = () => {
      socket = null;
      connectPromise = null;
      for (const [, entry] of pending) {
        entry.reject(new Error("Remote connection closed"));
      }
      pending.clear();
    };

    ws.onmessage = (event) => {
      if (typeof event.data === "string") {
        handleMessage(event.data);
      }
    };
  });

  return connectPromise;
}

export function rememberRemoteToken(token: string) {
  try {
    sessionStorage.setItem("monocode.remoteToken", token);
  } catch {
    // private mode / quota
  }
}

export function loadRemoteToken(): string | null {
  try {
    const fromQuery = new URLSearchParams(window.location.search).get("token");
    if (fromQuery) {
      rememberRemoteToken(fromQuery);
      return fromQuery;
    }
    return sessionStorage.getItem("monocode.remoteToken");
  } catch {
    return null;
  }
}

export function isRemoteSession(): boolean {
  return !isTauriRuntime();
}

export async function remoteInvoke<T>(
  command: string,
  args: Record<string, unknown> = {},
): Promise<T> {
  const token = loadRemoteToken();
  if (!token) {
    throw new Error("Missing remote access token");
  }

  await ensureSocket(token);
  if (!socket || socket.readyState !== WebSocket.OPEN) {
    throw new Error("Remote connection is not ready");
  }

  const id = nextId++;
  const payload = JSON.stringify({
    type: "invoke",
    id,
    command,
    args,
  });

  return new Promise<T>((resolve, reject) => {
    pending.set(id, {
      resolve: (value) => resolve(value as T),
      reject,
    });
    try {
      socket?.send(payload);
    } catch (error) {
      pending.delete(id);
      reject(error instanceof Error ? error : new Error(String(error)));
    }
  });
}

export function remoteListen<T>(
  event: string,
  handler: (event: { payload: T }) => void,
): Promise<() => void> {
  const wrapped: EventHandler = (payload) => handler({ payload: payload as T });
  const bucket = listeners.get(event) ?? new Set<EventHandler>();
  bucket.add(wrapped);
  listeners.set(event, bucket);

  return Promise.resolve(() => {
    bucket.delete(wrapped);
    if (bucket.size === 0) listeners.delete(event);
  });
}
