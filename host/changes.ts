import { randomUUID } from "node:crypto";
import type { SessionChange, SessionChanges } from "../src/features/connections/model/protocol";

const KEPT = 2_000;
// A streaming turn saves every 120 ms. Waiting briefly after the first change
// lets one response carry a burst instead of one response per save.
const COALESCE_MS = 40;
export const MAX_WAIT_MS = 25_000;

/**
 * In-memory log of session writes, so desktops learn about changes by waiting
 * on one request instead of polling every session. `boot` changes when the
 * host restarts; a desktop holding an old boot or a cursor older than the log
 * receives `reset` and reloads what it shows.
 */
export class ChangeFeed {
  readonly boot = randomUUID();
  private cursor = 0;
  private log: (SessionChange & { cursor: number })[] = [];
  private waiters = new Set<() => void>();

  record(change: SessionChange): void {
    this.log.push({ ...change, cursor: ++this.cursor });
    if (this.log.length > KEPT) this.log.splice(0, this.log.length - KEPT);
    for (const wake of [...this.waiters]) wake();
  }

  read(boot: unknown, after: unknown): SessionChanges {
    if (
      boot !== this.boot ||
      !Number.isSafeInteger(after) ||
      (after as number) < 0 ||
      (after as number) > this.cursor ||
      ((after as number) < this.cursor &&
        (this.log[0]?.cursor ?? Infinity) > (after as number) + 1)
    )
      return { boot: this.boot, cursor: this.cursor, sessions: [], reset: true };
    const latest = new Map<string, SessionChange>();
    for (const entry of this.log) {
      if (entry.cursor <= (after as number)) continue;
      const { cursor: _, ...change } = entry;
      latest.delete(change.id);
      latest.set(change.id, change);
    }
    return {
      boot: this.boot,
      cursor: this.cursor,
      sessions: [...latest.values()],
      reset: false,
    };
  }

  /** Resolves once changes after `after` exist, or with none at the timeout.
   * `signal` ends the wait early, such as when the desktop disconnects. */
  async wait(
    boot: unknown,
    after: unknown,
    timeout: number,
    signal?: AbortSignal,
  ): Promise<SessionChanges> {
    const now = this.read(boot, after);
    if (now.reset || now.sessions.length) return now;
    await new Promise<void>((resolve) => {
      let coalescing = false;
      const done = () => {
        clearTimeout(timer);
        this.waiters.delete(wake);
        signal?.removeEventListener("abort", done);
        resolve();
      };
      const wake = () => {
        if (coalescing) return;
        coalescing = true;
        clearTimeout(timer);
        timer = setTimeout(done, COALESCE_MS);
        timer.unref?.();
      };
      let timer = setTimeout(done, Math.max(0, Math.min(timeout, MAX_WAIT_MS)));
      timer.unref?.();
      this.waiters.add(wake);
      signal?.addEventListener("abort", done, { once: true });
    });
    return this.read(boot, after);
  }

  /** Releases every waiting request, such as when the host stops. */
  close(): void {
    for (const wake of [...this.waiters]) wake();
  }
}
