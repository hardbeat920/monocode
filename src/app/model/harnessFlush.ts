import type { HarnessEvent } from "../../integrations/harness/core/types";

export type ScheduledFlush = { kind: "raf" | "timeout"; id: number };

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
    return { kind: "timeout", id: window.setTimeout(run, 100) };
  }
  return { kind: "raf", id: requestAnimationFrame(run) };
}

/** Keep hidden streams on their own cadence even while a visible chat streams. */
export class HarnessEventQueue {
  private queued = new Map<string, HarnessEvent[]>();
  private foregroundFlush: ScheduledFlush | null = null;
  private backgroundFlush: ScheduledFlush | null = null;

  constructor(
    private readonly isForeground: (sessionId: string) => boolean,
    private readonly apply: (
      batches: ReadonlyMap<string, HarnessEvent[]>,
    ) => void,
  ) {}

  enqueue = (sessionId: string, event: HarnessEvent) => {
    const events = this.queued.get(sessionId);
    if (events) events.push(event);
    else this.queued.set(sessionId, [event]);
    if (
      event.type === "approval.requested" ||
      event.type === "approval.resolved" ||
      event.type === "question.asked" ||
      event.type === "question.resolved"
    ) {
      // Deliver the preceding output too, preserving this session's ordering.
      this.flushMatching((id) => id === sessionId);
      return;
    }
    this.schedule(this.visible(sessionId));
  };

  /** Lifecycle boundaries still need every session's latest output. */
  flush = () => this.flushMatching(() => true);

  /** Apply activity received during an asynchronous lifecycle checkpoint. */
  async flushAfter(checkpoint: Promise<void>): Promise<void> {
    await checkpoint;
    this.flush();
  }

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
