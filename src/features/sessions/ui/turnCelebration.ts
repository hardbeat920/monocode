import { useEffect, useLayoutEffect, useState, type RefObject } from "react";

// Only a turn sent moments ago celebrates; reopening an old thread stays calm.
const FRESH_MS = 4000;

// Remounts (tab switches, transcript windowing) must not replay a burst.
const celebrated = new Set<string>();

export function shouldCelebrateTurn(
  blockId: string,
  startedAt: number | undefined,
  now: number,
): boolean {
  if (startedAt == null || celebrated.has(blockId)) return false;
  const age = now - startedAt;
  return age >= 0 && age < FRESH_MS;
}

/** Whether a freshly sent turn's one-shot burst should be on screen now. */
export function useTurnCelebration(
  blockId: string,
  startedAt: number | undefined,
  durationMs: number,
): boolean {
  const [fresh] = useState(() =>
    shouldCelebrateTurn(blockId, startedAt, Date.now()),
  );
  const [done, setDone] = useState(false);

  useEffect(() => {
    if (!fresh) return;
    celebrated.add(blockId);
    const timer = setTimeout(() => setDone(true), durationMs);
    return () => clearTimeout(timer);
  }, [blockId, fresh, durationMs]);

  return fresh && !done;
}

export type CelebrationBox = { width: number; height: number };

/** The bubble's size, read before paint so a burst can lay itself out. */
export function useCelebrationBox(
  ref: RefObject<HTMLElement | null>,
  active: boolean,
): CelebrationBox | null {
  const [box, setBox] = useState<CelebrationBox | null>(null);

  useLayoutEffect(() => {
    if (!active || !ref.current) return;
    const { width, height } = ref.current.getBoundingClientRect();
    setBox({ width, height });
  }, [active, ref]);

  return box;
}
