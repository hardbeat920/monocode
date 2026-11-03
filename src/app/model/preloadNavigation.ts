/** Warm navigation code and data after the workspace has painted. */
export function preloadNavigationWhenIdle(
  preloaders: ReadonlyArray<() => Promise<unknown>>,
): () => void {
  let frame: number;
  let idle: number | undefined;
  let timeout: number | undefined;
  const preload = () => {
    for (const load of preloaders) {
      // A failed warmup must not prevent opening a page on demand.
      void load().catch(() => undefined);
    }
  };
  // Effects can run before paint; allow the workspace to appear first.
  frame = requestAnimationFrame(() => {
    frame = requestAnimationFrame(() => {
      if (typeof window.requestIdleCallback === "function") {
        idle = window.requestIdleCallback(preload, { timeout: 1000 });
      } else {
        timeout = window.setTimeout(preload, 0);
      }
    });
  });
  return () => {
    cancelAnimationFrame(frame);
    if (idle !== undefined) window.cancelIdleCallback(idle);
    if (timeout !== undefined) window.clearTimeout(timeout);
  };
}
