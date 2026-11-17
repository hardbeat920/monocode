import { backoff, pause, waitForActive } from "@/features/connection/appActivity";

import {
  follow,
  freshSync,
  newer,
  parseMessage,
  reportState,
  sendScript,
  trusted,
  type DraftAction,
  type DraftRecord,
  type LiveDraft,
  type PageCommand,
  type ShutDraft,
} from "./sync";

/** After a failed read, the phone asks again this much later. */
const RETRY_MS = 2000;
/** After repeated failures the phone waits longer, up to this. */
const RETRY_MAX_MS = 30000;
/** Taps and keys are sent together, at most this often. */
const TRAIL_MS = 600;
/** How often a card on screen looks again at a page that is not open. */
const SHUT_MS = 5000;
/** How often the phone asks for a new picture of a dev server. */
const FRAME_MS = 1000;

/** A card as the transcript carries it. */
export type DraftCardItem = { draft: string; title: string };

/** What the card shows in the page's place. */
export type DraftMode = "loading" | "page" | "frame" | "paused" | "closed";

/** All the card's own controls need, whichever platform draws them. */
export type DraftChrome = {
  mode: DraftMode;
  title: string;
  status: string | null;
  waiting: boolean;
  commenting: boolean;
  notes: number;
  handBack: boolean;
  /** The label of the button that opens the page, when it can open. */
  open: string | null;
  /** The page's height at a phone's width, until the page says itself. */
  height?: number;
};

/** The card on screen: Swift's on iOS, a React view on Android. */
export type DraftSurface = {
  chrome(chrome: DraftChrome): void;
  load(html: string, generation: number): void;
  script(script: string): void;
  /** A picture for the page's place; null clears it. */
  picture(uri: string | null, size?: { width: number; height: number }): void;
};

export type DraftPress = "comment" | "send" | "handBack" | "close" | "open";

type Command = <T>(method: string, params: Record<string, unknown>, id?: string) => Promise<T>;
type Press = { action: "send" } | { action: "handBack" } | { action: "close" };
type Frame = { live?: boolean; frame?: string; width?: number; height?: number };

const reason = (value: unknown) => (value instanceof Error ? value.message : String(value));

/**
 * One draft's card in a thread: what it shows, and its share of the desktop's
 * record while it is on screen. The rules for what crosses between the screens
 * are in `sync.ts`; this keeps them running against a card on either platform.
 *
 * A card looks at the desktop with `peek`, so scrolling past one never opens
 * its page there. Only the person's own press does.
 */
export class DraftController {
  private record: LiveDraft | null = null;
  private shut: ShutDraft | null = null;
  private error: string | null = null;
  private commenting = false;
  private sync = freshSync();
  /** The generation of the page the card has finished loading. */
  private loadedGeneration: number | null = null;
  /** The generation whose page the card holds, so the desktop leaves it out. */
  private held: number | null = null;
  private trail: string[] = [];
  private trailTimer: ReturnType<typeof setTimeout> | null = null;
  private stop: AbortController | null = null;
  private nap: AbortController | null = null;
  private lastChrome = "";
  private pictured = "";

  constructor(
    private options: {
      conversationId: string;
      card: DraftCardItem;
      light: boolean;
      command: Command;
      commandId: () => string;
      surface: DraftSurface;
      alert: (title: string, message: string) => void;
    },
  ) {
    this.emit();
  }

  /** The draft this card reads. */
  private get target() {
    return this.options.card.draft;
  }

  /** The transcript's item changed, as when the agent renames the draft. */
  setCard(card: DraftCardItem) {
    if (this.options.card.title === card.title) return;
    this.options = { ...this.options, card };
    this.emit();
  }

  /** The card came on screen: follow the record until `hide`. */
  show() {
    if (this.stop) return;
    const stop = new AbortController();
    this.stop = stop;
    void this.follow(stop);
  }

  /** The card left the screen. The page stays with it until `lost`. */
  hide() {
    this.stop?.abort();
    this.stop = null;
    this.nap?.abort();
  }

  /** The page itself is gone, so the desktop has to send it again. */
  lost() {
    this.held = null;
    this.loadedGeneration = null;
    this.sync = freshSync();
  }

  dispose() {
    this.hide();
    if (this.trailTimer) clearTimeout(this.trailTimer);
    this.trailTimer = null;
  }

  /** A page started loading, by the card's own doing or after a crash. */
  loadStarted() {
    this.loadedGeneration = null;
    this.sync = freshSync();
  }

  loaded(generation: number) {
    this.loadedGeneration = generation;
    if (this.commenting) this.send({ __mcComment: true });
    const live = this.record;
    if (live?.generation === generation)
      for (const message of follow(this.sync, live, Date.now())) this.send(message);
  }

  /** What the page said; `touchedAt` is the person's last touch that counts. */
  message(data: string, touchedAt: number) {
    const message = parseMessage(data);
    const live = this.record;
    if (!message || message.k === "h" || !live || this.loadedGeneration !== live.generation) return;
    const now = Date.now();
    // The page's own script can post anything; only what follows the
    // person's touch speaks for them.
    if (!trusted(message, touchedAt, this.commenting, now)) return;
    switch (message.k) {
      case "v": {
        const { page, desktop } = reportState(this.sync, live, message.v, touchedAt, now);
        page.forEach((command) => this.send(command));
        if (desktop) void this.act(desktop);
        break;
      }
      case "f":
        void this.act({ action: "pin", notes: [message.f] });
        break;
      case "u":
        if (message.a) this.trail.push(message.a);
        this.trailTimer ??= setTimeout(() => {
          this.trailTimer = null;
          void this.act({ action: "input", actions: this.trail.splice(0) });
        }, TRAIL_MS);
        break;
      case "n":
        void this.act({ action: "message", text: message.t });
        break;
    }
  }

  /**
   * The thread took the touch as a scroll. The page saw it start and said so,
   * but that was the person passing the card, not using it.
   */
  touchTaken() {
    this.trail.length = 0;
    if (this.trailTimer) clearTimeout(this.trailTimer);
    this.trailTimer = null;
  }

  press(button: DraftPress) {
    switch (button) {
      case "comment":
        this.setComment(!this.commenting);
        break;
      case "send":
      case "handBack":
      case "close":
        if (this.commenting) this.setComment(false);
        void this.act({ action: button });
        break;
      case "open":
        void this.open();
        break;
    }
  }

  private send(message: PageCommand) {
    this.options.surface.script(sendScript(message));
  }

  private setComment(on: boolean) {
    this.commenting = on;
    this.send({ __mcComment: on });
    this.emit();
  }

  private async open() {
    const { conversationId, light, command } = this.options;
    try {
      const reply = await command<DraftRecord>("draft.read", {
        conversationId,
        draft: this.target,
        light,
      });
      this.take(reply);
      this.nap?.abort();
    } catch (value) {
      this.options.alert("Could not reach the desktop", reason(value));
    }
  }

  private async act(action: DraftAction | Press) {
    const { conversationId, light, command, commandId } = this.options;
    const state = action.action === "state";
    if (state) this.sync.sending += 1;
    try {
      const reply = await command<DraftRecord>(
        "draft.action",
        {
          conversationId,
          draft: this.target,
          light,
          generation: this.held ?? undefined,
          ...action,
        },
        commandId(),
      );
      if (state) this.sync.sending -= 1;
      this.take(reply);
    } catch (value) {
      if (state) this.sync.sending -= 1;
      // A tap or a moved control that did not arrive is redone by hand;
      // notes and the buttons say so.
      if (action.action !== "input" && action.action !== "state")
        this.options.alert("Could not reach the desktop", reason(value));
    }
  }

  private take(reply: DraftRecord) {
    if (reply.live) this.apply(reply);
    else this.close(reply);
  }

  /** The page is not open on the desktop. */
  private close(reply: ShutDraft) {
    this.record = null;
    this.shut = reply;
    this.lost();
    if (this.commenting) this.commenting = false;
    this.emit();
  }

  private apply(next: LiveDraft) {
    const merged = newer(this.record, next);
    if (!merged.live) return;
    if (this.shut || this.pictured) {
      this.pictured = "";
      this.options.surface.picture(null);
    }
    this.shut = null;
    this.record = merged;
    this.emit();
    if (merged.page && this.held !== merged.generation) {
      this.held = merged.generation;
      this.loadedGeneration = null;
      this.sync = freshSync();
      this.options.surface.load(merged.page, merged.generation);
      return;
    }
    if (this.loadedGeneration === merged.generation)
      for (const message of follow(this.sync, merged, Date.now())) this.send(message);
  }

  private async rest(ms: number, stop: AbortController) {
    const nap = new AbortController();
    this.nap = nap;
    stop.signal.addEventListener("abort", () => nap.abort(), { once: true });
    await pause(ms, nap.signal);
  }

  /** Reads at once, then follows: each read waits until the draft changes. */
  private async follow(stop: AbortController) {
    const { conversationId, light, command } = this.options;
    const alive = () => !stop.signal.aborted;
    let failures = 0;
    while (alive()) {
      await waitForActive(stop.signal);
      if (!alive()) return;
      const known = this.record && this.held != null ? this.record : null;
      try {
        const reply = await command<DraftRecord>("draft.read", {
          conversationId,
          draft: this.target,
          light,
          peek: true,
          since: known?.rev,
          generation: this.held ?? undefined,
        });
        if (!alive()) return;
        failures = 0;
        this.error = null;
        if (reply.live) {
          this.apply(reply);
          if (reply.kind === "url") {
            await this.frame(stop);
            await this.rest(FRAME_MS, stop);
          }
          continue;
        }
        this.close(reply);
        await this.rest(SHUT_MS, stop);
      } catch (value) {
        if (!alive()) return;
        this.error = reason(value);
        this.emit();
        await this.rest(backoff(RETRY_MS, failures++, RETRY_MAX_MS), stop);
      }
    }
  }

  /** A dev server's page: the phone can't reach it, so it shows pictures. */
  private async frame(stop: AbortController) {
    const { conversationId, command, surface } = this.options;
    const reply = await command<Frame>("draft.frame", {
      conversationId,
      draft: this.target,
    });
    if (stop.signal.aborted || !reply.frame || reply.frame === this.pictured) return;
    this.pictured = reply.frame;
    surface.picture(
      reply.frame,
      reply.width && reply.height ? { width: reply.width, height: reply.height } : undefined,
    );
  }

  /** What the card's controls show now. */
  private chrome(): DraftChrome {
    const { card } = this.options;
    const title = this.record?.title || card.title;
    const base = {
      title,
      status: null,
      waiting: false,
      commenting: false,
      notes: 0,
      handBack: false,
      open: null,
    };
    if (this.shut) {
      if (this.shut.saved && !this.shut.dismissed)
        return { ...base, mode: "paused", open: "Paused · tap to run" };
      return {
        ...base,
        mode: "closed",
        status: `${title} was closed`,
        open: this.shut.saved ? "Open again" : null,
      };
    }
    const record = this.record;
    if (!record) return { ...base, mode: "loading", status: this.error };
    if (record.kind === "url")
      return {
        ...base,
        mode: "frame",
        title: record.url ? shortUrl(record.url) : title,
        status: "Live from your desktop",
      };
    const notes = record.pending.length;
    return {
      mode: "page",
      title,
      status: record.waiting
        ? "Waiting for you"
        : this.commenting
          ? "Tap the page to leave a note"
          : record.control && !notes
            ? "You're using the draft"
            : null,
      waiting: record.waiting,
      commenting: this.commenting,
      notes,
      handBack: record.control || record.waiting,
      open: null,
      height: record.height,
    };
  }

  private emit() {
    const chrome = this.chrome();
    const next = JSON.stringify(chrome);
    if (next === this.lastChrome) return;
    this.lastChrome = next;
    this.options.surface.chrome(chrome);
  }
}

/** The address without its scheme, as the person typed it. */
export const shortUrl = (url: string) => url.replace(/^https?:\/\//, "").replace(/\/$/, "");
