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

export function whenNativeTabReady(id: string, timeoutMs = 5000): Promise<void> {
  if (ready.has(id)) return Promise.resolve();
  return new Promise((resolve, reject) => {
    const list = waiters.get(id) ?? new Set<() => void>();
    waiters.set(id, list);
    const wake = () => {
      clearTimeout(timer);
      resolve();
    };
    const timer = setTimeout(() => {
      list.delete(wake);
      reject(new Error("The browser tab did not open"));
    }, timeoutMs);
    list.add(wake);
  });
}
