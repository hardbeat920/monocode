/** Tabs whose native page exists in this window. Agents wait on these. */
const ready = new Set<string>();
const waiters = new Map<string, Set<() => void>>();

export function setNativeTabReady(id: string, isReady: boolean) {
  if (!isReady) {
    ready.delete(id);
    return;
  }
  ready.add(id);
  for (const wake of waiters.get(id) ?? []) wake();
  waiters.delete(id);
}

export function isNativeTabReady(id: string): boolean {
  return ready.has(id);
}

export function whenNativeTabReady(
  id: string,
  timeoutMs = 5000,
  signal?: AbortSignal,
): Promise<void> {
  if (ready.has(id)) return Promise.resolve();
  if (signal?.aborted) return Promise.reject(signal.reason);
  return new Promise((resolve, reject) => {
    const list = waiters.get(id) ?? new Set<() => void>();
    waiters.set(id, list);
    const settle = () => {
      clearTimeout(timer);
      list.delete(wake);
      if (list.size === 0 && waiters.get(id) === list) waiters.delete(id);
      signal?.removeEventListener("abort", abort);
    };
    const wake = () => {
      settle();
      resolve();
    };
    const abort = () => {
      settle();
      reject(signal?.reason);
    };
    const timer = setTimeout(() => {
      settle();
      reject(new Error("The browser tab did not open"));
    }, timeoutMs);
    list.add(wake);
    signal?.addEventListener("abort", abort);
  });
}
