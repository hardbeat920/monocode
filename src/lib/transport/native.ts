import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { listen as tauriListen } from "@tauri-apps/api/event";
import type { WebSocketLike } from "./remote";

/**
 * Native (Rust) WebSocket, used inside Tauri. WKWebView pages are https, so
 * a JS `WebSocket` to ws://LAN is mixed content and never leaves the app.
 */

export type NativeWsEvent =
  | { kind: "open"; id: string }
  | { kind: "message"; id: string; data: string }
  | { kind: "close"; id: string; code: number; reason: string }
  | { kind: "error"; id: string; message: string };

export function canUseNativeSocket(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export function parseNativeWsEvent(value: unknown): NativeWsEvent | null {
  if (!value || typeof value !== "object") return null;
  if (!("kind" in value) || !("id" in value)) return null;
  const kind = value.kind;
  const id = value.id;
  if (typeof kind !== "string" || typeof id !== "string" || !id) return null;
  if (kind === "open") {
    return { kind: "open", id };
  }
  if (kind === "message") {
    if (!("data" in value) || typeof value.data !== "string") return null;
    return { kind: "message", id, data: value.data };
  }
  if (kind === "close") {
    if (!("code" in value) || typeof value.code !== "number") return null;
    const reason = "reason" in value && typeof value.reason === "string" ? value.reason : "";
    return { kind: "close", id, code: value.code, reason };
  }
  if (kind === "error") {
    if (!("message" in value) || typeof value.message !== "string") return null;
    return { kind: "error", id, message: value.message };
  }
  return null;
}

function newSocketId(): string {
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
    return crypto.randomUUID();
  }
  return `c-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

export function createNativeSocket(url: string): WebSocketLike {
  const id = newSocketId();
  let ready = false;
  let closed = false;
  const queued: string[] = [];
  let unlisten: (() => void) | null = null;

  const socket: WebSocketLike = {
    send(data: string) {
      if (closed) return;
      if (!ready) {
        queued.push(data);
        return;
      }
      void tauriInvoke("companion_ws_send", { id, data });
    },
    close(code = 1000, reason = "") {
      if (closed) return;
      closed = true;
      ready = false;
      unlisten?.();
      unlisten = null;
      void tauriInvoke("companion_ws_close", { id });
      socket.onclose?.({ code, reason });
    },
    onopen: null,
    onmessage: null,
    onclose: null,
    onerror: null,
  };

  void (async () => {
    try {
      unlisten = await tauriListen("companion-ws", (event) => {
        const payload = parseNativeWsEvent(event.payload);
        if (!payload || payload.id !== id || closed) return;
        switch (payload.kind) {
          case "open":
            ready = true;
            for (const data of queued.splice(0)) {
              void tauriInvoke("companion_ws_send", { id, data });
            }
            socket.onopen?.({});
            return;
          case "message":
            socket.onmessage?.({ data: payload.data });
            return;
          case "error":
            socket.onerror?.(payload);
            return;
          case "close":
            closed = true;
            ready = false;
            unlisten?.();
            unlisten = null;
            socket.onclose?.({ code: payload.code, reason: payload.reason });
            return;
          default: {
            const _exhaustive: never = payload;
            void _exhaustive;
          }
        }
      });
      if (closed) {
        unlisten?.();
        unlisten = null;
        return;
      }
      await tauriInvoke("companion_ws_open", { id, url });
    } catch (error) {
      if (closed) return;
      closed = true;
      socket.onerror?.(error);
      socket.onclose?.({
        code: 1006,
        reason: error instanceof Error ? error.message : "open failed",
      });
      unlisten?.();
      unlisten = null;
    }
  })();

  return socket;
}
