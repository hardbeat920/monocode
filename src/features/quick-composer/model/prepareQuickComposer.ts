import { invoke } from "@tauri-apps/api/core";

/** Prepare hidden windows after a paint and in idle time, never on startup's
 * critical path. If focus leaves first, wait for the workspace to regain it. */
export function prepareQuickComposerWhenIdle(): () => void {
  let disposed = false;
  let prepared = false;
  let inFlight = false;
  let frame: number | undefined;
  let idle: number | undefined;
  let timeout: number | undefined;
  const foreground = () => !document.hidden && document.hasFocus();
  const cancel = () => {
    if (frame !== undefined) cancelAnimationFrame(frame);
    if (idle !== undefined) window.cancelIdleCallback(idle);
    if (timeout !== undefined) window.clearTimeout(timeout);
    frame = idle = timeout = undefined;
  };
  const prepare = () => {
    idle = timeout = undefined;
    if (disposed || prepared || inFlight || !foreground()) return;
    inFlight = true;
    void invoke<boolean>("quick_composer_prepare")
      .then((ready) => {
        prepared = ready;
      })
      .catch(() => {
        /* Retry on the next focus; drafting can still open on demand. */
      })
      .finally(() => {
        inFlight = false;
      });
  };
  const schedule = () => {
    cancel();
    if (disposed || prepared || inFlight || !foreground()) return;
    // React effects may run before paint. Two frames allow the main interface
    // to paint before we request idle work from the browser.
    frame = requestAnimationFrame(() => {
      frame = requestAnimationFrame(() => {
        frame = undefined;
        if (typeof window.requestIdleCallback === "function") {
          idle = window.requestIdleCallback(prepare);
        } else {
          timeout = window.setTimeout(prepare, 0);
        }
      });
    });
  };
  window.addEventListener("focus", schedule);
  window.addEventListener("blur", cancel);
  document.addEventListener("visibilitychange", schedule);
  schedule();
  return () => {
    disposed = true;
    cancel();
    window.removeEventListener("focus", schedule);
    window.removeEventListener("blur", cancel);
    document.removeEventListener("visibilitychange", schedule);
  };
}
