import {
  closeWatch,
  nextUpdates,
  openWatch,
  request,
  type Pairing,
} from "@/features/connection/nativeLink";

import { fromBase64, toBase64 } from "./base64";

/** One update on a terminal's stream, as the desktop sends it. */
export type TerminalFrame = {
  shellId: string;
  /** Where the output stands after this frame, in bytes since the shell began. */
  seq: number;
  /** Base64 output, passed to the terminal view as it is. */
  data?: string;
  /** The first frame: clear the screen first, unless carrying on from before. */
  reset?: boolean;
  /** The first frame: the sequence the next keystrokes continue from. */
  next?: number;
  exit?: number;
  error?: string;
};

export type TerminalState = "connecting" | "live" | "reconnecting" | "ended";

export type TerminalTarget = { projectId: string; worktreePath?: string };

type Handlers = {
  /** Output to draw; `reset` clears the screen before it. */
  onOutput: (data: string, reset: boolean) => void;
  onState: (state: TerminalState, detail?: string) => void;
};

/** A shell to come back to: after the phone leaves and returns. */
export type Resume = { shellId: string; seq: number; next: number };

const RETRY_MS = [500, 1000, 2000, 4000, 8000];
/** Typing held back while the desktop can't be reached, in bytes. */
const MAX_PENDING = 64 * 1024;
/**
 * Keystroke requests on their way at once. The desktop puts up to 64 back in
 * order, so typing need not wait a round trip per key.
 */
const MAX_IN_FLIGHT = 32;
/** Before typing that did not get through is sent again. */
const RETYPE_MS = 1000;
const POLL_MS = 20_000;

/**
 * A shell on the desktop, followed over a watch. Output arrives as frames the
 * terminal view draws; typing goes back as numbered requests the desktop puts
 * in order, so a retry never types anything twice.
 */
export class TerminalSession {
  private shellId: string | null = null;
  private seq = 0;
  private nextInput = 1;
  private watchId: number | null = null;
  private pending: Uint8Array[] = [];
  private pendingBytes = 0;
  /** Sent typing not yet acknowledged, by sequence, to send again if lost. */
  private inflight = new Map<number, string>();
  private queued = false;
  private retry: ReturnType<typeof setTimeout> | null = null;
  private closed = false;
  private size: { cols: number; rows: number };
  /** The size the desktop's shell has, so a resize is sent only when it changed. */
  private sizeSent: { cols: number; rows: number } | null = null;

  constructor(
    private readonly pairing: Pairing,
    private readonly target: TerminalTarget,
    size: { cols: number; rows: number },
    private readonly handlers: Handlers,
    resume?: Resume,
  ) {
    this.size = size;
    if (resume) {
      this.shellId = resume.shellId;
      this.seq = resume.seq;
      this.nextInput = resume.next;
    }
  }

  /** What to hand `resume` to come back to this shell. */
  get resume(): Resume | null {
    return this.shellId
      ? { shellId: this.shellId, seq: this.seq, next: this.nextInput }
      : null;
  }

  async start() {
    let attempt = 0;
    while (!this.closed) {
      this.handlers.onState(attempt === 0 && !this.shellId ? "connecting" : "reconnecting");
      try {
        await this.follow();
        attempt = 0;
      } catch (error) {
        if (this.closed) return;
        const message = error instanceof Error ? error.message : String(error);
        // The desktop is up and says the shell is gone, or terminals are off:
        // retrying cannot help.
        if (/ended|turned off|revoked|not paired|no longer/i.test(message)) {
          this.finish(message);
          return;
        }
        if (attempt >= RETRY_MS.length * 2) {
          this.finish(message);
          return;
        }
        await sleep(RETRY_MS[Math.min(attempt, RETRY_MS.length - 1)]);
        attempt += 1;
      }
    }
  }

  /** Types bytes into the shell. */
  input(bytes: Uint8Array) {
    if (this.closed || bytes.length === 0) return;
    if (this.pendingBytes + bytes.length > MAX_PENDING) return;
    this.pending.push(bytes);
    this.pendingBytes += bytes.length;
    // Keys typed in the same tick (a paste, an action) go in one request.
    if (this.queued) return;
    this.queued = true;
    queueMicrotask(() => {
      this.queued = false;
      this.flush();
    });
  }

  resize(cols: number, rows: number) {
    this.size = { cols, rows };
    void this.sendSize();
  }

  /** Ends the shell on the desktop. Leaving the screen without this keeps it. */
  async close() {
    this.closed = true;
    if (this.retry) clearTimeout(this.retry);
    const watch = this.watchId;
    this.watchId = null;
    if (watch !== null) await closeWatch(watch);
    if (this.shellId)
      await request(this.pairing, "terminal.close", {
        shellId: this.shellId,
      }).catch(() => {});
    this.handlers.onState("ended");
  }

  /**
   * Draws the shell again from the start of what the desktop kept: the screen
   * was lost and is empty. The watch reopens by itself.
   */
  replay() {
    if (this.closed || !this.shellId) return;
    this.seq = 0;
    const watch = this.watchId;
    this.watchId = null;
    if (watch !== null) void closeWatch(watch);
  }

  /** Stops following, leaving the shell running to come back to. */
  detach() {
    this.closed = true;
    if (this.retry) clearTimeout(this.retry);
    const watch = this.watchId;
    this.watchId = null;
    if (watch !== null) void closeWatch(watch);
  }

  private finish(detail?: string) {
    this.closed = true;
    this.handlers.onState("ended", detail);
  }

  private async follow() {
    const watch = await openWatch(this.pairing, "terminal.watch", {
      projectId: this.target.projectId,
      ...(this.target.worktreePath ? { worktreePath: this.target.worktreePath } : {}),
      ...(this.shellId ? { shellId: this.shellId, after: this.seq } : {}),
      cols: this.size.cols,
      rows: this.size.rows,
    });
    if (this.closed) {
      void closeWatch(watch);
      return;
    }
    this.watchId = watch;
    this.sizeSent = this.size;
    try {
      for (;;) {
        const frames = await nextUpdates<TerminalFrame>(watch, POLL_MS);
        if (this.closed) return;
        for (const frame of frames) this.apply(frame);
        this.flush();
      }
    } finally {
      if (this.watchId === watch) this.watchId = null;
      void closeWatch(watch);
    }
  }

  private apply(frame: TerminalFrame) {
    if (frame.error) throw new Error(frame.error);
    this.shellId = frame.shellId;
    if (frame.next !== undefined) this.carryOn(frame.next);
    if (frame.data || frame.reset) this.handlers.onOutput(frame.data ?? "", frame.reset === true);
    this.seq = frame.seq;
    this.handlers.onState("live");
    // A phone that rotated while away tells the shell on its return.
    if (frame.reset !== undefined) void this.sendSize();
    if (frame.exit !== undefined) this.finish(`Exited with code ${frame.exit}`);
  }

  /**
   * The desktop expects keystroke `next` after (re)attaching: what was sent
   * below it got there, and the rest goes again under the same numbers.
   */
  private carryOn(next: number) {
    for (const seq of this.inflight.keys()) if (seq < next) this.inflight.delete(seq);
    this.nextInput = Math.max(next, ...[...this.inflight.keys()].map((seq) => seq + 1));
    if (this.retry) clearTimeout(this.retry);
    this.retry = null;
    for (const [seq, data] of this.inflight) this.send(seq, data);
  }

  private flush() {
    if (this.closed || !this.shellId || this.watchId === null || this.retry) return;
    if (this.pending.length === 0 || this.inflight.size >= MAX_IN_FLIGHT) return;
    const bytes = new Uint8Array(this.pendingBytes);
    let at = 0;
    for (const chunk of this.pending) {
      bytes.set(chunk, at);
      at += chunk.length;
    }
    this.pending = [];
    this.pendingBytes = 0;
    const seq = this.nextInput++;
    const data = toBase64(bytes);
    this.inflight.set(seq, data);
    this.send(seq, data);
  }

  private send(seq: number, data: string) {
    const shellId = this.shellId;
    if (!shellId) return;
    request(this.pairing, "terminal.input", { shellId, seq, data }).then(
      () => {
        this.inflight.delete(seq);
        this.flush();
      },
      (error: unknown) => {
        // Held for the reconnect; the same sequence is safe to send again.
        if (this.closed || !this.inflight.has(seq)) return;
        const message = error instanceof Error ? error.message : String(error);
        if (/ended|turned off|revoked|not paired/i.test(message)) this.finish(message);
        else this.retry ??= setTimeout(() => this.resend(), RETYPE_MS);
      },
    );
  }

  private resend() {
    this.retry = null;
    if (this.closed) return;
    if (!this.shellId || this.watchId === null) {
      this.retry = setTimeout(() => this.resend(), RETYPE_MS);
      return;
    }
    for (const [seq, data] of this.inflight) this.send(seq, data);
    this.flush();
  }

  private async sendSize() {
    const size = this.size;
    if (
      !this.shellId ||
      this.watchId === null ||
      (this.sizeSent?.cols === size.cols && this.sizeSent?.rows === size.rows)
    )
      return;
    this.sizeSent = size;
    await request(this.pairing, "terminal.resize", {
      shellId: this.shellId,
      ...size,
    }).catch(() => {
      this.sizeSent = null;
    });
  }
}

function sleep(ms: number) {
  return new Promise<void>((resolve) => setTimeout(resolve, ms));
}

export { fromBase64 };
