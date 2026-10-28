import type { Element, ElementContent, Root } from "hast";
import { useEffect, useReducer, useRef, useState } from "react";

/*
 * Streaming prose, paced. Tokens land in uneven bursts; read straight off the
 * wire, whole clauses pop in at once and then nothing for a beat. Instead the
 * text is let out a word at a time at a steady rate that closes on whatever
 * has arrived, and each word fades in as it is let out (the fade itself is
 * `.word-fading [data-word-fade]` in index.css), so the reply grows with a
 * soft leading edge rather than a ragged one.
 */

/** How long one word takes to fade in; matches `word-fade-in` in index.css. */
export const WORD_FADE_MS = 320;
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
 * The part of `text` to show right now. Text that is already there when the
 * component mounts, or that changes while nothing is streaming, shows at
 * once; only what streams in is paced, and a stream that ends ahead of the
 * reveal is still let out at pace. `revealing` stays true until the reveal
 * has caught up.
 */
export function usePacedText(
  text: string,
  streaming: boolean,
): { text: string; revealing: boolean } {
  const shown = useRef(text.length);
  const pacing = useRef(streaming);
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
    let hold = 0;
    let frame = requestAnimationFrame(function tick(now) {
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
      if (position < text.length) frame = requestAnimationFrame(tick);
      else if (shown.current < text.length) {
        hold = window.setTimeout(() => {
          shown.current = text.length;
          rerender();
        }, REVEAL_HOLD_MS);
      }
    });
    return () => {
      cancelAnimationFrame(frame);
      window.clearTimeout(hold);
    };
  }, [text, streaming, behind]);

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
export function useWordFading(active: boolean): boolean {
  const [lingering, setLingering] = useState(false);

  useEffect(() => {
    if (active) {
      setLingering(true);
      return;
    }
    const timer = window.setTimeout(() => setLingering(false), WORD_FADE_MS);
    return () => window.clearTimeout(timer);
  }, [active]);

  return active || lingering;
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
