import type { Element, ElementContent, Root } from "hast";
import { useEffect, useReducer, useRef, useState } from "react";

/*
 * Streaming prose. Tokens land in uneven bursts. By default each burst goes
 * on screen as it arrives, so layout is always the real reply, and its words
 * fade in over a time that follows the stream cadance
 * veil): a fast stream gets short fades, a slow one long fades, and either
 * way consecutive bursts overlap into one rolling edge instead of popping in
 * clause by clause. The fade itself is `.word-fading [data-word-fade]` in
 * index.css.
 *
 * The older paced reveal, which lets text out a word at a time at a steady
 * rate, stays available behind `localStorage["monocode.pacedReveal"] = "1"`.
 */

/** How long one word takes to fade in while paced. */
export const WORD_FADE_MS = 320;

/** Opt back into the paced word-at-a-time reveal. */
export const PACED_REVEAL_KEY = "monocode.pacedReveal";

/**
 * Fade length tracks an average of the gaps between bursts: three gaps long,
 * within these bounds.
 */
const CADENCE_FADE_MIN_MS = 120;
const CADENCE_FADE_MAX_MS = 400;
/** The gap average a fresh stream starts from. */
const CADENCE_SEED_MS = 160;
/** A stall longer than this counts as this long, so one pause can't drag every fade out. */
const CADENCE_GAP_CAP_MS = 1000;

const CADENCE_FADE_EASE = "cubic-bezier(0.33, 0.53, 0.67, 1)";

export function pacedRevealPreferred(): boolean {
  try {
    return localStorage.getItem(PACED_REVEAL_KEY) === "1";
  } catch {
    return false;
  }
}

/** The next gap average once a burst lands `gapMs` after the one before. */
export function nextCadence(averageMs: number, gapMs: number): number {
  return averageMs * 0.7 + Math.min(gapMs, CADENCE_GAP_CAP_MS) * 0.3;
}

/** How long a burst's words fade, given the current gap average. */
export function cadenceFadeMs(averageMs: number): number {
  return Math.min(
    CADENCE_FADE_MAX_MS,
    Math.max(CADENCE_FADE_MIN_MS, averageMs * 3),
  );
}

/**
 * The fade length and curve for words appearing in `text` now. Each change
 * that adds to the text is one burst; the gap since the previous one feeds
 * the average. A render that repeats the same text (strict mode, an unrelated
 * parent update) leaves it alone.
 */
export function useCadenceFade(text: string): {
  durationMs: number;
  easing: string;
} {
  const state = useRef({
    text,
    average: CADENCE_SEED_MS,
    lastBurst: null as number | null,
  });
  const current = state.current;
  if (text !== current.text) {
    if (text.length > current.text.length) {
      const now = performance.now();
      if (current.lastBurst !== null)
        current.average = nextCadence(current.average, now - current.lastBurst);
      current.lastBurst = now;
    }
    current.text = text;
  }
  return {
    durationMs: Math.round(cadenceFadeMs(current.average)),
    easing: CADENCE_FADE_EASE,
  };
}
/**
 * The slowest the reveal goes, in characters a second, so the tail of a
 * finished reply never crawls out.
 */
const REVEAL_MIN_CPS = 90;
/**
 * The reveal closes on what has arrived over about this long, so a steady
 * stream runs this far behind the wire and a burst spreads over it.
 */
const REVEAL_CATCHUP_S = 0.22;
/**
 * How long a word still being written is held back once the reveal has
 * caught up to it. Past this the stream has paused on it, so it shows as is.
 */
const REVEAL_HOLD_MS = 150;

/**
 * Where to stop revealing `text` for a reveal that has reached `at`: the end
 * of the word `at` falls in, so a word is never shown half written. A stream
 * still mid-word holds back at the last whole word; a finished one runs out.
 */
export function revealEnd(
  text: string,
  at: number,
  streaming: boolean,
): number {
  for (let i = Math.max(0, Math.ceil(at)); i < text.length; i++) {
    if (isSpace(text.charCodeAt(i))) return i;
  }
  if (!streaming) return text.length;
  let end = text.length;
  while (end > 0 && !isSpace(text.charCodeAt(end - 1))) end--;
  return end;
}

function isSpace(code: number): boolean {
  return code === 32 || code === 10 || code === 9 || code === 13;
}

/**
 * Show streamed output as it arrives, holding back only a word still being
 * written until it is whole or the stream pauses on it. `revealing` stays on
 * for the first render of new output so it fades in, including a reply that
 * completed before its first paint; callers opening existing output opt out.
 */
export function useStreamedText(
  text: string,
  streaming: boolean,
  revealOnMount = streaming,
): { text: string; revealing: boolean } {
  const [mountReveal, setMountReveal] = useState(revealOnMount);
  // Output already there when it opens is shown whole, half-written word too.
  const [heldText, setHeldText] = useState<string | null>(
    revealOnMount ? null : text,
  );
  const end =
    heldText === text ? text.length : revealEnd(text, text.length, streaming);
  const behind = end < text.length;

  useEffect(() => {
    if (mountReveal) setMountReveal(false);
  }, [mountReveal]);

  useEffect(() => {
    if (!behind) return;
    const timer = window.setTimeout(() => setHeldText(text), REVEAL_HOLD_MS);
    return () => window.clearTimeout(timer);
  }, [behind, text]);

  return {
    text: behind ? text.slice(0, end) : text,
    revealing: behind || mountReveal,
  };
}

/**
 * Pace new output, including its first chunk and a reply that completed before
 * its first paint. Callers opening existing output opt out of the mount reveal.
 * A stream that ends ahead of the reveal still finishes at pace.
 */
export function usePacedText(
  text: string,
  streaming: boolean,
  revealOnMount = streaming,
): { text: string; revealing: boolean } {
  const shown = useRef(revealOnMount ? 0 : text.length);
  const pacing = useRef(streaming || revealOnMount);
  const latest = useRef({ text, streaming });
  latest.current = { text, streaming };
  const [, rerender] = useReducer((n: number) => n + 1, 0);

  if (streaming) pacing.current = true;
  if (!pacing.current) shown.current = text.length;
  shown.current = Math.min(shown.current, text.length);
  const behind = shown.current < text.length;

  useEffect(() => {
    if (!pacing.current) return;
    if (!behind) {
      if (!streaming) pacing.current = false;
      return;
    }
    let position = shown.current;
    let last = performance.now();
    let heldText = "";
    let heldAt = 0;
    let frame = requestAnimationFrame(function tick(now) {
      const { text, streaming } = latest.current;
      const dt = Math.min(0.05, (now - last) / 1000);
      last = now;
      const backlog = text.length - position;
      const speed = Math.max(REVEAL_MIN_CPS, backlog / REVEAL_CATCHUP_S);
      position = Math.min(text.length, position + speed * dt);
      const end = revealEnd(text, position, streaming);
      if (end > shown.current) {
        shown.current = end;
        rerender();
      }
      // Once the reveal has run into the end of what has arrived there is
      // nothing to do until more does, which restarts this.
      if (position >= text.length && shown.current < text.length) {
        if (heldText !== text) {
          heldText = text;
          heldAt = now;
        }
        if (now - heldAt >= REVEAL_HOLD_MS) {
          shown.current = text.length;
          rerender();
        }
      }
      if (shown.current < text.length) frame = requestAnimationFrame(tick);
    });
    return () => cancelAnimationFrame(frame);
    // Incoming chunks update the target without restarting the reveal clock.
  }, [streaming, behind]);

  return {
    text: behind ? text.slice(0, shown.current) : text,
    revealing: behind,
  };
}

/**
 * Whether a reply's words may fade: while it streams or is being let out, and
 * for one fade after, so the last word finishes. Outside that the fade is off
 * — an animation replays whenever its element is hidden and shown again, and
 * a finished reply folded away and reopened must not fade in all over again.
 */
export function useWordFading(active: boolean, fadeMs = WORD_FADE_MS): boolean {
  const [lingering, setLingering] = useState(false);
  const lastFadeMs = useRef(fadeMs);
  lastFadeMs.current = fadeMs;

  useEffect(() => {
    if (active) {
      setLingering(true);
      return;
    }
    const timer = window.setTimeout(
      () => setLingering(false),
      lastFadeMs.current,
    );
    return () => window.clearTimeout(timer);
  }, [active]);

  return active || lingering;
}

/**
 * Marks every word on screen under `root` as already faded in. Showing a
 * `display: none` subtree restarts its CSS animations, so a reply still
 * fading when its tab was hidden would otherwise fade in whole again. Words
 * let out after this are new spans and fade as usual.
 */
export function settleWordFades(root: ParentNode) {
  for (const word of root.querySelectorAll(
    ".word-fading [data-word-fade]:not([data-word-settled])",
  )) {
    word.setAttribute("data-word-settled", "");
  }
}

/*
 * Text here is either not prose (code, math, drawings) or read whole by the
 * component that renders it (links).
 */
const UNFADED_TAGS = new Set(["a", "code", "pre", "svg", "math", "kbd"]);

/**
 * Wraps every word in a span that fades in as it is added to the page. The
 * plugin keeps no state: a word already on screen keeps its element however
 * often its block re-renders, so it never fades twice, and a word the reveal
 * has just let out is a new element, so it does. That holds because every
 * word gets a span — the JSX keys count spans, and a word without one would
 * shift the keys of every word after it and fade them again.
 */
export function rehypeWordFade() {
  return (tree: Root) => {
    const wrap = (value: string): ElementContent[] =>
      value
        .split(/(\s+)/)
        .filter(Boolean)
        .map((part) =>
          isSpace(part.charCodeAt(0))
            ? { type: "text", value: part }
            : {
                type: "element",
                tagName: "span",
                properties: { dataWordFade: "" },
                children: [{ type: "text", value: part }],
              },
        );

    const walk = (parent: Root | Element) => {
      const children: Array<Root["children"][number]> = [];
      for (const child of parent.children) {
        if (child.type === "text") children.push(...wrap(child.value));
        else {
          if (child.type === "element" && !UNFADED_TAGS.has(child.tagName)) {
            walk(child);
          }
          children.push(child);
        }
      }
      parent.children = children as typeof parent.children;
    };

    walk(tree);
  };
}
