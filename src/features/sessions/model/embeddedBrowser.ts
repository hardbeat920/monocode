import { invoke } from "@tauri-apps/api/core";

export type BrowserBounds = {
  x: number;
  y: number;
  width: number;
  height: number;
};
export type BrowserRequest =
  | { action: "open"; url: string; bounds: BrowserBounds }
  | { action: "layout"; bounds: BrowserBounds; visible: boolean }
  | { action: "back" | "forward" | "reload" | "close" | "url" };

// One queue per app webview. Close must follow an in-flight creation, including
// React StrictMode's setup/cleanup/setup cycle and rapid close/reopen clicks.
let pending: Promise<unknown> = Promise.resolve();
export function browserRequest(
  request: BrowserRequest,
): Promise<string | null> {
  const result = pending.then(() =>
    invoke<string | null>("embedded_browser", { request }),
  );
  pending = result.catch(() => undefined);
  return result;
}

export function browserBounds(element: HTMLElement): BrowserBounds {
  const rect = element.getBoundingClientRect();
  const scale = window.devicePixelRatio || 1;
  return {
    x: Math.max(0, Math.round(rect.x * scale)),
    y: Math.max(0, Math.round(rect.y * scale)),
    width: Math.max(1, Math.round(rect.width * scale)),
    height: Math.max(1, Math.round(rect.height * scale)),
  };
}
