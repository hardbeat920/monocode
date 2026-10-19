import type { HarnessEvent } from "../../integrations/harness/core/types";
import { noteStreamArrival } from "../../features/sessions/model/streamingPerformance";

export type ScheduledFlush = { kind: "raf" | "timeout"; id: number };

/**
 * Visible content subscribers advance at ~30Hz; paced text animates between
 * deliveries independently of the app shell.
 * Just under two 60Hz frames, so frame jitter can't halve the rate.
 */
export const FOREGROUND_FLUSH_MS = 30;
export const BACKGROUND_FLUSH_MS = 250;

export function cancelScheduledFlush(handle: ScheduledFlush | null) {
  if (!handle) return;
  if (handle.kind === "raf") cancelAnimationFrame(handle.id);
  else clearTimeout(handle.id);
}

/** Hidden output must keep advancing without driving the whole UI at 60–120Hz. */
export function scheduleHarnessFlush(
  run: () => void,
  foreground: boolean,
): ScheduledFlush {
  if (document.hidden || !foreground) {
    return { kind: "timeout", id: window.setTimeout(run, BACKGROUND_FLUSH_MS) };
  }
  return { kind: "raf", id: requestAnimationFrame(run) };
}

/** Keep hidden streams on their own cadence even while a visible chat streams. */
export class HarnessEventQueue {
  private queued = new Map<string, HarnessEvent[]>();
  private foregroundFlush: ScheduledFlush | null = null;
  private backgroundFlush: ScheduledFlush | null = null;
  private lastForegroundFlush = -Infinity;

  constructor(
    /** Whether the conversation is on screen; idle parking reads it too. */
    readonly isForeground: (sessionId: string) => boolean,
    private readonly apply: (
      batches: ReadonlyMap<string, HarnessEvent[]>,
    ) => void,
  ) {}

  enqueue = (sessionId: string, event: HarnessEvent) => {
    const foreground = this.visible(sessionId);
    noteStreamArrival(event, foreground);
    const events = this.queued.get(sessionId);
    if (events) events.push(event);
    else this.queued.set(sessionId, [event]);
    if (
      event.type === "approval.requested" ||
      event.type === "approval.resolved" ||
      event.type === "question.asked" ||
      event.type === "question.updated" ||
      event.type === "question.resolved" ||
      event.type === "session.ended" ||
      event.type === "session.error" ||
      event.type === "turn.ready" ||
      event.type === "usage.limited"
    ) {
      // Deliver the preceding output too, preserving this session's ordering.
      this.flushMatching((id) => id === sessionId);
      return;
    }
    this.schedule(foreground);
  };

  /** Lifecycle boundaries still need every session's latest output. */
  flush = () => this.flushMatching(() => true);

  /** Tab activation catches up its visible panes without flushing hidden work. */
  flushForeground = () => this.flushMatching((id) => this.visible(id));

  cancelScheduled = () => {
    cancelScheduledFlush(this.foregroundFlush);
    cancelScheduledFlush(this.backgroundFlush);
    this.foregroundFlush = null;
    this.backgroundFlush = null;
  };

  private visible(sessionId: string) {
    return !document.hidden && this.isForeground(sessionId);
  }

  private flushMatching(matches: (sessionId: string) => boolean) {
    const batches = new Map<string, HarnessEvent[]>();
    for (const [id, events] of this.queued) {
      if (!matches(id)) continue;
      batches.set(id, events);
      this.queued.delete(id);
    }
    this.schedulePending();
    if (batches.size > 0) this.apply(batches);
  }

  private schedulePending() {
    let foreground = false;
    let background = false;
    for (const id of this.queued.keys()) {
      if (this.visible(id)) foreground = true;
      else background = true;
      if (foreground && background) break;
    }
    if (!foreground) {
      cancelScheduledFlush(this.foregroundFlush);
      this.foregroundFlush = null;
    } else this.schedule(true);
    if (!background) {
      cancelScheduledFlush(this.backgroundFlush);
      this.backgroundFlush = null;
    } else this.schedule(false);
  }

  private schedule(foreground: boolean) {
    if (foreground) {
      if (this.foregroundFlush) return;
      this.foregroundFlush = scheduleHarnessFlush(() => {
        this.foregroundFlush = null;
        const now = performance.now();
        if (now - this.lastForegroundFlush < FOREGROUND_FLUSH_MS) {
          this.schedule(true);
          return;
        }
        this.lastForegroundFlush = now;
        this.flushForeground();
      }, true);
    } else if (!this.backgroundFlush) {
      this.backgroundFlush = scheduleHarnessFlush(() => {
        this.backgroundFlush = null;
        this.flushMatching((id) => !this.visible(id));
      }, false);
    }
  }
}
