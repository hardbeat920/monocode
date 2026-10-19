import type { Element, ElementContent, Root } from "hast";
import { useEffect, useReducer, useRef, useState } from "react";
import { loadPacedReveal } from "../../settings/model/displayPrefs";
export { PACED_REVEAL_KEY } from "../../settings/model/displayPrefs";

/*
 * Smooth uneven provider bursts locally, independently of app-shell updates.
 * Already loaded text appears immediately. The unpaced fallback remains
 * available with localStorage["monocode.pacedReveal"] = "0".
 */

/** How long one word takes to fade in while paced. */
export const WORD_FADE_MS = 320;

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
  if (typeof window === "undefined") return false;
  try {
    return (
      !window.matchMedia?.("(prefers-reduced-motion: reduce)").matches &&
      loadPacedReveal()
    );
  } catch {
    return true;
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
// Parsing is CPU work, unlike compositor word fades. Cap commits on high
// refresh displays; advance by elapsed time when the main thread stalls.
export const REVEAL_FRAME_MS = 30;

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
  active = true,
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
    if (!behind || !active) return;
    const timer = window.setTimeout(() => setHeldText(text), REVEAL_HOLD_MS);
    return () => window.clearTimeout(timer);
  }, [active, behind, text]);

  return {
    text: behind && active ? text.slice(0, end) : text,
    revealing: active && (behind || mountReveal),
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
  active = true,
): { text: string; revealing: boolean } {
  const shown = useRef(revealOnMount ? 0 : text.length);
  const pacing = useRef(streaming || revealOnMount);
  const latest = useRef({ text, streaming });
  latest.current = { text, streaming };
  const [, rerender] = useReducer((n: number) => n + 1, 0);
  const wasActive = useRef(active);

  if (streaming) pacing.current = true;
  // Unseen output is history on activation, never a reveal backlog.
  if (
    !active ||
    !wasActive.current ||
    (typeof document !== "undefined" && document.hidden)
  )
    shown.current = text.length;
  wasActive.current = active;
  if (!pacing.current) shown.current = text.length;
  shown.current = Math.min(shown.current, text.length);
  const behind = shown.current < text.length;

  useEffect(() => {
    if (!active || !pacing.current) return;
    if (!behind) {
      if (!streaming) pacing.current = false;
      return;
    }
    let position = shown.current;
    let last = performance.now();
    let heldText = "";
    let heldAt = 0;
    let frame = 0;
    const finishHidden = () => {
      if (!document.hidden) return;
      cancelAnimationFrame(frame);
      shown.current = latest.current.text.length;
      rerender();
    };
    const tick = (now: number) => {
      const { text, streaming } = latest.current;
      if (document.hidden) {
        finishHidden();
        return;
      }
      const elapsed = now - last;
      if (elapsed < REVEAL_FRAME_MS) {
        frame = requestAnimationFrame(tick);
        return;
      }
      const dt = elapsed / 1000;
      last = now;
      const backlog = text.length - position;
      const advance = Math.max(
        REVEAL_MIN_CPS * dt,
        backlog * (1 - Math.exp(-dt / REVEAL_CATCHUP_S)),
      );
      position = Math.min(text.length, position + advance);
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
    };
    frame = requestAnimationFrame(tick);
    document.addEventListener("visibilitychange", finishHidden);
    return () => {
      cancelAnimationFrame(frame);
      document.removeEventListener("visibilitychange", finishHidden);
    };
    // Incoming chunks update the target without restarting the reveal clock.
  }, [active, streaming, behind]);

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

/**
 * Settles the words under `root` now and those the tab's catch-up adds next.
 * A hidden transcript stops rendering, and Streamdown commits the text that
 * streamed meanwhile in a transition after the tab is shown, so those words
 * would otherwise all fade in at once on return. A mutation observer runs
 * before the browser paints, so none of them is seen fading. It lets go a
 * frame after the catch-up lands, so words that stream in after that fade.
 */
export function settleCatchUpFades(root: ParentNode & Node): () => void {
  settleWordFades(root);
  let frame = 0;
  const observer = new MutationObserver(() => {
    settleWordFades(root);
    cancelAnimationFrame(frame);
    frame = requestAnimationFrame(stop);
  });
  const timer = window.setTimeout(stop, CATCH_UP_LIMIT_MS);
  function stop() {
    observer.disconnect();
    cancelAnimationFrame(frame);
    window.clearTimeout(timer);
  }
  observer.observe(root, { childList: true, subtree: true });
  return stop;
}

/** The longest a returning tab waits for its catch-up render to land. */
const CATCH_UP_LIMIT_MS = 500;

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
      // Streamdown memoizes prose elements by source position and className,
      // ignoring changes to their children. Mark every ancestor while the
      // plugin is active so removing the spans updates those elements in
      // place, rather than remounting entire lists and their code/media.
      if (parent.type === "element") {
        const classes = parent.properties.className;
        parent.properties.className = [
          ...(Array.isArray(classes)
            ? classes
            : classes
              ? [String(classes)]
              : []),
          "word-fade-content",
        ];
      }
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
