import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useRef, type RefObject } from "react";
import { previewUrl } from "../lib/browserPreview";

export type PreviewEvent = {
  id: string;
  kind:
    "loading" | "loaded" | "url" | "focus" | "blocked" | "popup" | "download";
  url: string;
};
type Action = "back" | "forward" | "reload" | { navigate: string };
type Controller = {
  navigate: (url: string) => void;
  action: (action: Action) => void;
};

export function useBrowserPreview(
  host: RefObject<HTMLDivElement | null>,
  url: string,
  onEvent: (event: PreviewEvent) => void,
  onError: (error: string) => void,
) {
  const controller = useRef<Controller | null>(null);
  const callbacks = useRef({ onEvent, onError });
  useEffect(() => {
    callbacks.current = { onEvent, onError };
  }, [onEvent, onError]);

  useEffect(() => {
    const element = host.current;
    if (!element) return;
    const id = crypto.randomUUID();
    let closed = false;
    let created = false;
    let creationFailed = false;
    let desiredUrl = "";
    let openedUrl = "";
    let geometry = "";
    let dragging = false;
    let frame = 0;
    let polling = false;
    const listening = listen<PreviewEvent>("browser-preview", ({ payload }) => {
      if (!closed && payload.id === id) callbacks.current.onEvent(payload);
    });
    let queue: Promise<unknown> = listening;
    const enqueue = (task: () => Promise<unknown>) => {
      queue = queue.then(task).catch((error: unknown) => {
        if (!closed) callbacks.current.onError(String(error));
      });
    };

    const bounds = () => {
      if (
        document.hidden ||
        dragging ||
        element.closest('[inert], [aria-hidden="true"]')
      )
        return null;
      // A native view sits above HTML. Suspend it while application overlays
      // are present so dialogs, menus and approval buttons remain usable.
      const overlay = [
        ...document.querySelectorAll(
          '[role="dialog"], [data-popover-side], .approval-toast',
        ),
      ].some((node) => node.getClientRects().length > 0);
      if (
        overlay ||
        document.documentElement.classList.contains("is-reordering")
      )
        return null;
      const rect = element.getBoundingClientRect();
      if (rect.width < 1 || rect.height < 1) return null;
      return {
        x: rect.x,
        y: rect.y,
        width: rect.width,
        height: rect.height,
        viewportWidth: window.innerWidth,
      };
    };
    const sync = () =>
      enqueue(async () => {
        await listening;
        if (closed) return;
        const next = bounds();
        if (!created) {
          if (!next || !desiredUrl || creationFailed) return;
          const initialUrl = desiredUrl;
          try {
            await invoke("browser_preview_open", {
              id,
              url: initialUrl,
              bounds: next,
            });
          } catch (error) {
            creationFailed = true;
            throw error;
          }
          created = true;
          openedUrl = initialUrl;
          geometry = "";
        }
        if (desiredUrl && desiredUrl !== openedUrl) {
          await invoke("browser_preview_action", {
            id,
            action: { navigate: desiredUrl },
          });
          openedUrl = desiredUrl;
        }
        const serialized = JSON.stringify(next);
        if (serialized !== geometry) {
          await invoke("browser_preview_sync", { id, bounds: next });
          geometry = serialized;
        }
      });
    const schedule = () => {
      if (frame || closed) return;
      frame = requestAnimationFrame(() => {
        frame = 0;
        sync();
      });
    };
    const current: Controller = {
      navigate(value) {
        const target = previewUrl(value);
        if (!target) {
          callbacks.current.onError("Enter an HTTP or HTTPS address.");
          return;
        }
        desiredUrl = target;
        creationFailed = false;
        sync();
      },
      action(action) {
        if (!created) {
          creationFailed = false;
          sync();
          return;
        }
        enqueue(async () => {
          if (!closed && created)
            await invoke("browser_preview_action", { id, action });
        });
      },
    };
    controller.current = current;
    const resize = new ResizeObserver(schedule);
    resize.observe(element);
    const mutations = new MutationObserver(schedule);
    mutations.observe(document.body, {
      subtree: true,
      childList: true,
      attributes: true,
      attributeFilter: ["class", "style", "inert", "aria-hidden"],
    });
    const down = () => {
      dragging = true;
      sync();
    };
    const up = () => {
      dragging = false;
      schedule();
    };
    window.addEventListener("pointerdown", down, true);
    window.addEventListener("pointerup", up, true);
    window.addEventListener("pointercancel", up, true);
    window.addEventListener("resize", schedule);
    document.addEventListener("visibilitychange", schedule);
    const interval = setInterval(() => {
      if (closed || !created || polling || !bounds()) return;
      polling = true;
      enqueue(async () => {
        try {
          if (closed) return;
          const currentUrl = await invoke<string>("browser_preview_url", {
            id,
          });
          if (!closed && currentUrl)
            callbacks.current.onEvent({ id, kind: "url", url: currentUrl });
        } finally {
          polling = false;
        }
      });
    }, 1000);
    return () => {
      closed = true;
      controller.current = null;
      cancelAnimationFrame(frame);
      clearInterval(interval);
      resize.disconnect();
      mutations.disconnect();
      window.removeEventListener("pointerdown", down, true);
      window.removeEventListener("pointerup", up, true);
      window.removeEventListener("pointercancel", up, true);
      window.removeEventListener("resize", schedule);
      document.removeEventListener("visibilitychange", schedule);
      // Close only after an in-flight create has finished; StrictMode/remounts
      // use different ids and cannot close the replacement's view.
      void queue
        .then(async () => {
          if (created) await invoke("browser_preview_close", { id });
        })
        .catch((error: unknown) =>
          console.error("Could not close preview", error),
        );
      void listening
        .then((unlisten) => unlisten())
        .catch((error: unknown) =>
          console.error("Could not release preview listener", error),
        );
    };
  }, [host]);

  useEffect(() => {
    if (url) controller.current?.navigate(url);
  }, [url]);
  return (action: Action) => controller.current?.action(action);
}
