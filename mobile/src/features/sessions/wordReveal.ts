import { useEffect, useLayoutEffect, useState } from "react";

// Streaming prose, as the desktop's wordFade.tsx: each burst goes on screen as
// it arrives, a word still being written waits until it is whole, and new
// words fade in over a time that follows the stream's cadence, so bursts
// overlap into one rolling edge instead of popping in clause by clause.

/** Fade length tracks an average of the gaps between bursts: three gaps long, within these bounds. */
const CADENCE_FADE_MIN_MS = 120;
const CADENCE_FADE_MAX_MS = 400;
/** The gap average a fresh stream starts from. */
const CADENCE_SEED_MS = 160;
/** A stall longer than this counts as this long, so one pause can't drag every fade out. */
const CADENCE_GAP_CAP_MS = 1000;
/** How long a word still being written is held back before it shows as is. */
const REVEAL_HOLD_MS = 150;

/** The next gap average once a burst lands `gapMs` after the one before. */
export function nextCadence(averageMs: number, gapMs: number): number {
  return averageMs * 0.7 + Math.min(gapMs, CADENCE_GAP_CAP_MS) * 0.3;
}

/** How long a burst's words fade, given the current gap average. */
export function cadenceFadeMs(averageMs: number): number {
  return Math.min(CADENCE_FADE_MAX_MS, Math.max(CADENCE_FADE_MIN_MS, averageMs * 3));
}

/**
 * Where to stop showing `text`: the end of the word `at` falls in, so a word
 * is never shown half written. A stream mid-word holds back at the last whole
 * word; a finished one runs out.
 */
export function revealEnd(text: string, at: number, streaming: boolean): number {
  for (let i = Math.max(0, Math.ceil(at)); i < text.length; i++) {
    if (isSpace(text.charCodeAt(i))) return i;
  }
  if (!streaming) return text.length;
  let end = text.length;
  while (end > 0 && !isSpace(text.charCodeAt(end - 1))) end--;
  return end;
}

export function isSpace(code: number): boolean {
  return code === 32 || code === 10 || code === 9 || code === 13;
}

/**
 * Show streamed text as it arrives, holding back only a word still being
 * written until it is whole or the stream pauses on it.
 */
export function useStreamedText(text: string, streaming: boolean): string {
  // Text already there when it opens is shown whole, half-written word too.
  const [heldText, setHeldText] = useState<string | null>(text);
  const end = heldText === text ? text.length : revealEnd(text, text.length, streaming);
  const behind = end < text.length;
  useEffect(() => {
    if (!behind) return;
    const timer = setTimeout(() => setHeldText(text), REVEAL_HOLD_MS);
    return () => clearTimeout(timer);
  }, [behind, text]);
  return behind ? text.slice(0, end) : text;
}

/**
 * The fade length for new words. Each time the reply grows is one burst; the
 * gap since the one before feeds the average.
 */
export class Cadence {
  private average = CADENCE_SEED_MS;
  private lastBurst: number | null = null;
  burst(now: number): void {
    if (this.lastBurst !== null) this.average = nextCadence(this.average, now - this.lastBurst);
    this.lastBurst = now;
  }
  get fadeMs(): number {
    return Math.round(cadenceFadeMs(this.average));
  }
}

/**
 * When each word went on screen, by its place in the reply. Words there when
 * the reply opened count as long settled; later ones fade from their arrival.
 */
export class WordClock {
  private born: number[] = [];
  private opened = false;
  /** Stamps words up to `count`; true when new ones went on screen to fade. */
  stamp(count: number, now: number, animate: boolean): boolean {
    if (count < this.born.length) this.born.length = count;
    const at = this.opened || animate ? now : -Infinity;
    const grew = this.born.length < count && at !== -Infinity;
    while (this.born.length < count) this.born.push(at);
    this.opened = true;
    return grew;
  }
  /** 0 to 1: how far word `index` has faded in at `now`. */
  alpha(index: number, now: number, fadeMs: number): number {
    const born = this.born[index];
    if (born === undefined) return 1;
    const t = (now - born) / fadeMs;
    return t >= 1 ? 1 : t <= 0 ? 0 : fadeEase(t);
  }
  /** Whether any word is still fading at `now`. */
  fading(now: number, fadeMs: number): boolean {
    const last = this.born[this.born.length - 1];
    return last !== undefined && now - last < fadeMs;
  }
  /** The first word still fading at `now`, so settled ones render as plain text. */
  firstFading(now: number, fadeMs: number): number {
    let index = this.born.length;
    while (index > 0 && now - this.born[index - 1] < fadeMs) index--;
    return index;
  }
}

/** The desktop's fade curve, `cubic-bezier(0.33, 0.53, 0.67, 1)`. */
export function fadeEase(t: number): number {
  return cubicBezier(0.33, 0.53, 0.67, 1, t);
}

function cubicBezier(x1: number, y1: number, x2: number, y2: number, x: number): number {
  const at = (a1: number, a2: number, t: number) =>
    3 * a1 * (1 - t) * (1 - t) * t + 3 * a2 * (1 - t) * t * t + t * t * t;
  // Solve x(t) = x by bisection; the curve is monotonic in x.
  let low = 0;
  let high = 1;
  let t = x;
  for (let i = 0; i < 20; i++) {
    const value = at(x1, x2, t);
    if (Math.abs(value - x) < 1e-4) break;
    if (value < x) low = t;
    else high = t;
    t = (low + high) / 2;
  }
  return at(y1, y2, t);
}

/**
 * Closes emphasis and code the stream has opened but not yet closed, as the
 * desktop's remend does, so `**bo` streams in bold instead of as stars.
 */
export function closeOpenMarkers(text: string): string {
  const lastBlock = text.slice(text.lastIndexOf("\n\n") + 1);
  // Inside an open fence everything is literal.
  if ((text.match(/^ {0,3}(```|~~~)/gm)?.length ?? 0) % 2 === 1) return text;
  let tail = "";
  const ticks = lastBlock.match(/`/g)?.length ?? 0;
  if (ticks % 2 === 1) return text + "`";
  const plain = lastBlock.replace(/`[^`]*`/g, "");
  if ((plain.match(/\*\*/g)?.length ?? 0) % 2 === 1) tail += "**";
  return tail && !/\s$/.test(text) ? text + tail : text;
}

/**
 * Fades a reply's words in as they arrive: stamps new words before they
 * paint, then re-renders each frame until the newest has faded in.
 */
export function useWordFade(count: number, animate: boolean) {
  const [clock] = useState(() => new WordClock());
  const [cadence] = useState(() => new Cadence());
  const [frame, setFrame] = useState({ now: 0, fadeMs: cadence.fadeMs, active: false });
  // Before paint, so a new word never shows at full strength first.
  useLayoutEffect(() => {
    const now = performance.now();
    if (clock.stamp(count, now, animate)) cadence.burst(now);
    const fadeMs = cadence.fadeMs;
    setFrame({ now, fadeMs, active: clock.fading(now, fadeMs) });
    // `animate` only decides the words there on mount.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [count, clock, cadence]);
  useEffect(() => {
    if (!frame.active) return;
    let id = requestAnimationFrame(function tick() {
      const now = performance.now();
      setFrame((current) => ({ ...current, now, active: clock.fading(now, current.fadeMs) }));
      id = requestAnimationFrame(tick);
    });
    return () => cancelAnimationFrame(id);
  }, [frame.active, clock]);
  return { clock, now: frame.now, fadeMs: frame.fadeMs };
}
