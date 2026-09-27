import { useEffect, useState, type CSSProperties } from "react";

// Longest mark delay plus duration, with a little slack for the fade.
const BURST_MS = 1900;
const MARK_COUNT = 16;

export type BurstRect = {
  left: number;
  top: number;
  width: number;
  height: number;
};

type Mark = {
  glyph: boolean;
  x: number;
  size: number;
  delay: number;
  duration: number;
  drift: number;
  rise: number;
  tilt: number;
};

function makeMarks(): Mark[] {
  return Array.from({ length: MARK_COUNT }, (_, i) => {
    const glyph = i % 4 !== 3;
    return {
      glyph,
      // Spread across the composer with jitter so it never reads as a grid.
      x: Math.min(95, Math.max(5, ((i + Math.random()) / MARK_COUNT) * 100)),
      size: glyph ? 11 + Math.random() * 9 : 4 + Math.random() * 3,
      delay: 120 + Math.random() * 520,
      duration: 900 + Math.random() * 500,
      drift: (Math.random() - 0.5) * 36,
      rise: 110 + Math.random() * 130,
      tilt: (Math.random() < 0.5 ? -1 : 1) * (10 + Math.random() * 18),
    };
  }).sort(() => Math.random() - 0.5);
}

/**
 * A one-shot burst for opening a side question: question marks drift up out
 * of the composer into the panel as it opens, while the composer box glows
 * and a sheen crosses it. The /operator sparkles' cousin, in the accent color.
 */
export function BtwQuestionBurst({
  rect,
  onDone,
}: {
  rect: BurstRect;
  onDone: () => void;
}) {
  const [marks] = useState(makeMarks);

  useEffect(() => {
    const timer = setTimeout(onDone, BURST_MS);
    return () => clearTimeout(timer);
    // One burst per mount; a new open remounts with a new key.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <div
      aria-hidden
      className="btw-burst"
      style={{
        left: rect.left,
        top: rect.top,
        width: rect.width,
        height: rect.height,
      }}
    >
      <span className="btw-burst-sheen" />
      {marks.map((mark, index) => (
        <span
          key={index}
          className="btw-burst-mark"
          style={
            {
              "--x": `${mark.x}%`,
              "--size": `${mark.size}px`,
              "--delay": `${mark.delay}ms`,
              "--duration": `${mark.duration}ms`,
              "--drift": `${mark.drift}px`,
              "--rise": `${mark.rise}px`,
              "--tilt": `${mark.tilt}deg`,
            } as CSSProperties
          }
        >
          {mark.glyph ? (
            <span className="btw-burst-glyph">?</span>
          ) : (
            <span className="btw-burst-glyph btw-burst-ember" />
          )}
        </span>
      ))}
    </div>
  );
}
