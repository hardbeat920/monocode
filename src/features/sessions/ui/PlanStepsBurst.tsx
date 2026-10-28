import { useMemo, useRef, type CSSProperties } from "react";
import { useCelebrationBox, useTurnCelebration } from "./turnCelebration";

// Last mark's delay plus its flight through the bubble, with fade slack.
const CELEBRATE_MS = 3200;
const FIRST_STEP_MS = 250;
const STEP_GAP_MS = 170;
const STEP_RISE_MS = 1650;
const EMBER_COUNT = 6;
const CHECK_PATH = "M3.2 6.3 5.1 8.2 8.8 4.2";

/** Roomy bubbles get more steps; a one-word prompt still gets a short plan. */
export function planStepCount(width: number): number {
  return Math.max(2, Math.min(6, Math.floor(width / 64)));
}

function stepX(index: number, count: number): number {
  return ((index + 0.5) / count) * 100;
}

function makeEmbers(stepSpanMs: number) {
  return Array.from({ length: EMBER_COUNT }, (_, i) => ({
    x: Math.min(95, Math.max(5, ((i + Math.random()) / EMBER_COUNT) * 100)),
    size: 2.5 + Math.random() * 2,
    delay: FIRST_STEP_MS + Math.random() * (stepSpanMs + 300),
    duration: 1000 + Math.random() * 600,
    drift: (Math.random() - 0.5) * 20,
  }));
}

/** Numbered plan marks tick off as they float up and out of a fresh turn. */
export function PlanStepsBurst({
  blockId,
  startedAt,
}: {
  blockId: string;
  startedAt?: number;
}) {
  const ref = useRef<HTMLSpanElement>(null);
  const active = useTurnCelebration(blockId, startedAt, CELEBRATE_MS);
  const box = useCelebrationBox(ref, active);
  const count = box ? planStepCount(box.width) : 0;
  const spanMs = (count - 1) * STEP_GAP_MS;
  // Parent re-renders mid-burst must not reshuffle the embers.
  const embers = useMemo(() => makeEmbers(spanMs), [spanMs]);

  if (!active) return null;
  return (
    <span aria-hidden ref={ref} className="plan-steps">
      {box ? (
        <>
          {Array.from({ length: count }, (_, index) => (
            <span
              key={index}
              className="plan-step"
              style={
                {
                  "--x": `${stepX(index, count)}%`,
                  "--delay": `${FIRST_STEP_MS + index * STEP_GAP_MS}ms`,
                  "--duration": `${STEP_RISE_MS}ms`,
                  "--drift": `${index % 2 === 0 ? -8 : 8}px`,
                } as CSSProperties
              }
            >
              <span className="plan-step-dot">
                <span className="plan-step-number">{index + 1}</span>
                <svg viewBox="0 0 12 12" className="plan-step-check">
                  <path d={CHECK_PATH} />
                </svg>
              </span>
            </span>
          ))}
          {embers.map((ember, index) => (
            <span
              key={`ember-${index}`}
              className="plan-step-ember"
              style={
                {
                  "--x": `${ember.x}%`,
                  "--size": `${ember.size}px`,
                  "--delay": `${ember.delay}ms`,
                  "--duration": `${ember.duration}ms`,
                  "--drift": `${ember.drift}px`,
                } as CSSProperties
              }
            >
              <span className="plan-step-ember-glyph" />
            </span>
          ))}
        </>
      ) : null}
    </span>
  );
}
