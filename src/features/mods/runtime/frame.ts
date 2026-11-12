import { convertFileSrc } from "@tauri-apps/api/core";
import type { HostToWorker, WorkerToHost } from "../api/types";

/** What the bootstrap page relays: worker messages, plus a worker crash. */
export type FrameMessage = WorkerToHost | { kind: "crash"; message: string };

export type ModFrame = {
  post(message: HostToWorker): void;
  destroy(): void;
};

/**
 * Mounts the mod's hidden `monomod://` frame. It has an opaque origin and no
 * IPC, so messages are trusted only when they come from this frame's window.
 */
export function mountFrame(
  name: string,
  onMessage: (message: FrameMessage) => void,
): Promise<ModFrame> {
  const frame = document.createElement("iframe");
  frame.hidden = true;
  frame.setAttribute("sandbox", "allow-scripts");
  frame.setAttribute("aria-hidden", "true");
  frame.src = convertFileSrc(name, "monomod");
  const listener = (e: MessageEvent) => {
    if (e.source !== frame.contentWindow) return;
    const data = e.data as FrameMessage | null;
    if (data && typeof data === "object" && typeof data.kind === "string")
      onMessage(data);
  };
  window.addEventListener("message", listener);
  const handle: ModFrame = {
    post: (message) => frame.contentWindow?.postMessage(message, "*"),
    destroy() {
      window.removeEventListener("message", listener);
      frame.remove();
    },
  };
  return new Promise((resolve, reject) => {
    frame.addEventListener("load", () => resolve(handle), { once: true });
    frame.addEventListener(
      "error",
      () => {
        handle.destroy();
        reject(new Error(`Could not load the frame for ${name}`));
      },
      { once: true },
    );
    document.body.appendChild(frame);
  });
}
