import { useEffect, useState, type CSSProperties } from "react";

// Only a turn sent moments ago celebrates; reopening an old thread stays calm.
const FRESH_MS = 4000;
// Longest sparkle delay plus duration, with a little slack for the fade.
const CELEBRATE_MS = 3200;
const SPARKLE_COUNT = 18;
const STAR_PATH =
  "M12 0C12.9 6.6 17.4 11.1 24 12C17.4 12.9 12.9 17.4 12 24C11.1 17.4 6.6 12.9 0 12C6.6 11.1 11.1 6.6 12 0Z";

// Remounts (tab switches, transcript windowing) must not replay the burst.
const celebrated = new Set<string>();

type Sparkle = {
  star: boolean;
  x: number;
  size: number;
  delay: number;
  duration: number;
  drift: number;
  spin: number;
};

export function shouldCelebrateMonocode(
  blockId: string,
  startedAt: number | undefined,
  now: number,
): boolean {
  if (startedAt == null || celebrated.has(blockId)) return false;
  const age = now - startedAt;
  return age >= 0 && age < FRESH_MS;
}

function makeSparkles(): Sparkle[] {
  return Array.from({ length: SPARKLE_COUNT }, (_, i) => {
    const star = i % 3 !== 2;
    return {
      star,
      // Spread evenly across the bubble with some jitter so it never reads as a grid.
      x: Math.min(96, Math.max(4, ((i + Math.random()) / SPARKLE_COUNT) * 100)),
      size: star ? 7 + Math.random() * 6 : 2.5 + Math.random() * 2,
      delay: 150 + Math.random() * 950,
      duration: 1200 + Math.random() * 800,
      drift: (Math.random() - 0.5) * 28,
      spin: (Math.random() < 0.5 ? -1 : 1) * (90 + Math.random() * 120),
    };
  }).sort(() => Math.random() - 0.5);
}

/** A one-shot burst of rising sparkles inside a freshly sent /operator bubble. */
export function MonocodeSparkles({
  blockId,
  startedAt,
}: {
  blockId: string;
  startedAt?: number;
}) {
  const [sparkles] = useState(() =>
    shouldCelebrateMonocode(blockId, startedAt, Date.now())
      ? makeSparkles()
      : null,
  );
  const [done, setDone] = useState(false);

  useEffect(() => {
    if (!sparkles) return;
    celebrated.add(blockId);
    const timer = setTimeout(() => setDone(true), CELEBRATE_MS);
    return () => clearTimeout(timer);
  }, [blockId, sparkles]);

  if (!sparkles || done) return null;
  return (
    <span aria-hidden className="monocode-sparkles">
      {sparkles.map((sparkle, index) => (
        <span
          key={index}
          className="monocode-sparkle"
          style={
            {
              "--x": `${sparkle.x}%`,
              "--size": `${sparkle.size}px`,
              "--delay": `${sparkle.delay}ms`,
              "--duration": `${sparkle.duration}ms`,
              "--drift": `${sparkle.drift}px`,
              "--spin": `${sparkle.spin}deg`,
            } as CSSProperties
          }
        >
          {sparkle.star ? (
            <svg viewBox="0 0 24 24" className="monocode-sparkle-glyph">
              <path d={STAR_PATH} fill="currentColor" />
            </svg>
          ) : (
            <span className="monocode-sparkle-glyph monocode-sparkle-ember" />
          )}
        </span>
      ))}
    </span>
  );
}
