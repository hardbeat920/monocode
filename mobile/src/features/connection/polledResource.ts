import { isAppActive, onAppActive, backoff } from "./appActivity";

export type PolledOptions<T> = {
  /** The held value when `next` is no different from it, else `next`; the poller counts a read that returns the held value as unchanged. */
  merge: (held: T | null, next: T) => T;
  /** Milliseconds between reads. */
  baseMs?: number;
  /** The longest wait after failed reads. */
  errorMaxMs?: number;
};

type Holder<T> = { fetch: () => Promise<T> };

/**
 * One poller shared by everything reading the same thing (say a project's
 * changes in one worktree). It reads only while someone holds it (their screen
 * is up and focused) and the app is in the foreground, and waits longer after
 * failures. A screen that takes hold of it reads at once, so what it shows is
 * never stale on return. What it last read stays for the next screen to show
 * while that read lands.
 */
export class PolledResource<T> {
  private value: T | null = null;
  private error: string | null = null;
  private snapshot: { value: T | null; error: string | null } = { value: null, error: null };
  private listeners = new Set<() => void>();
  private holders = new Set<Holder<T>>();
  private timer: ReturnType<typeof setTimeout> | null = null;
  private stopActive: (() => void) | null = null;
  private inflight = false;
  private again = false;
  private failures = 0;
  private lastAt = 0;
  /** Reads started, for tests and measuring. */
  reads = 0;

  constructor(private options: PolledOptions<T>) {}

  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };

  getSnapshot = () => this.snapshot;

  /** Keeps the poller running until the returned function is called. */
  hold(fetch: () => Promise<T>): () => void {
    const holder: Holder<T> = { fetch };
    this.holders.add(holder);
    if (this.holders.size === 1) this.start();
    // A screen that comes up, or back into focus, reads now rather than
    // after the rest of the interval; a read already under way serves it.
    else if (!this.inflight && isAppActive()) this.runNow();
    return () => {
      if (!this.holders.delete(holder)) return;
      if (this.holders.size === 0) this.stop();
    };
  }

  /** Reads again at once, say after a commit. */
  reload = () => {
    this.failures = 0;
    if (this.holders.size === 0 || !isAppActive()) return;
    if (this.inflight) this.again = true;
    else this.runNow();
  };

  private start() {
    this.stopActive = onAppActive(() => {
      // Back in the foreground, read if the last read is old.
      if (this.holders.size > 0 && !this.inflight) this.schedule();
    });
    if (isAppActive()) this.runNow();
  }

  private stop() {
    this.stopActive?.();
    this.stopActive = null;
    if (this.timer) clearTimeout(this.timer);
    this.timer = null;
  }

  private delay() {
    const { baseMs = 10000, errorMaxMs = 60000 } = this.options;
    return this.failures > 0 ? backoff(baseMs, this.failures, errorMaxMs) : baseMs;
  }

  /** Waits out the rest of the interval since the last read, then reads. */
  private schedule() {
    if (this.timer) clearTimeout(this.timer);
    this.timer = null;
    if (this.holders.size === 0 || !isAppActive()) return;
    const wait = this.lastAt ? Math.max(0, this.lastAt + this.delay() - Date.now()) : 0;
    this.timer = setTimeout(() => {
      this.timer = null;
      void this.poll();
    }, wait);
  }

  private runNow() {
    if (this.timer) clearTimeout(this.timer);
    this.timer = null;
    void this.poll();
  }

  private async poll() {
    const holder = this.holders.values().next().value;
    if (!holder || this.inflight || !isAppActive()) return;
    this.inflight = true;
    this.reads++;
    try {
      const next = await holder.fetch();
      const merged = this.options.merge(this.value, next);
      this.failures = 0;
      this.value = merged;
      this.publish(merged, null);
    } catch (reason) {
      this.failures++;
      this.publish(this.value, reason instanceof Error ? reason.message : String(reason));
    } finally {
      this.inflight = false;
      this.lastAt = Date.now();
      if (this.again) {
        this.again = false;
        this.runNow();
      } else this.schedule();
    }
  }

  private publish(value: T | null, error: string | null) {
    if (value === this.snapshot.value && error === this.snapshot.error) return;
    this.snapshot = { value, error };
    for (const listener of [...this.listeners]) listener();
  }
}

const registry = new Map<string, PolledResource<unknown>>();

/** The shared poller for `key`, made on first use and kept for the app's life. */
export function polledResource<T>(key: string, options: PolledOptions<T>): PolledResource<T> {
  let found = registry.get(key) as PolledResource<T> | undefined;
  if (!found) {
    found = new PolledResource(options);
    registry.set(key, found as PolledResource<unknown>);
  }
  return found;
}

/** For tests. */
export function resetPolledResources() {
  registry.clear();
}
