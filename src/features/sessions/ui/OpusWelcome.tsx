import {
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
} from "react";
import { opusStage } from "../model/opusWelcome";
import "./OpusWelcome.css";

const DURATION_MS = 7000;

// A short phrase: it climbs, hesitates, and resolves on the middle line.
const MELODY = [
  [0.06, -3],
  [0.13, -1],
  [0.2, 1],
  [0.28, 0],
  [0.36, 2],
  [0.44, 4],
  [0.52, 3],
  [0.6, 1],
  [0.68, 2],
  [0.77, 0],
] as const;

type Stage = { top: number; width: number; height: number };

/**
 * Lays the scene out in stage pixels: five lines cross the stage edge to edge,
 * rising as a loose, handwritten wave and gathering into a single thread.
 */
function sceneLayout({ width, height }: Stage) {
  const endY = height * 0.42;
  const rise = height * 0.86 - endY;
  const staffY = (t: number, step: number) =>
    endY +
    rise * (1 - t) ** 1.7 +
    Math.sin(t * Math.PI * 2.4 + 0.3) * Math.min(20, height * 0.08) * (1 - t) -
    (step / 2) * 11 * (1 - 0.84 * t);

  const lines = [-4, -2, 0, 2, 4].map((step) =>
    Array.from({ length: 121 }, (_, index) => {
      const t = index / 120;
      return `${index ? "L" : "M"}${(t * width).toFixed(1)} ${staffY(t, step).toFixed(1)}`;
    }).join(" "),
  );
  const notes = MELODY.map(([t, step], index) => ({
    style: {
      "--note-x": `${t * width}px`,
      "--note-y": `${staffY(t, step)}px`,
      "--note-size": `${12.6 * (1 - 0.84 * t)}px`,
      "--note-delay": `${1.05 + index * 0.23}s`,
    } as CSSProperties,
    stemDown: step >= 1,
  }));
  return { lines, notes };
}

const MOTES = Array.from(
  { length: 26 },
  (_, index) =>
    ({
      "--mote-x": `${4 + ((index * 41) % 92)}%`,
      "--mote-y": `${20 + ((index * 29) % 72)}%`,
      "--mote-size": `${2 + (index % 4)}px`,
      "--mote-delay": `${0.5 + ((index * 11) % 30) * 0.12}s`,
      "--mote-duration": `${2.6 + (index % 5) * 0.4}s`,
      "--mote-drift": `${(index % 2 ? 1 : -1) * (6 + (index % 7) * 3)}px`,
    }) as CSSProperties,
);

/**
 * A decorative layer confined to the session, with no input interception.
 * The glow covers the pane; the staff plays in the space by the composer.
 */
export function OpusWelcome({ onDone }: { onDone: () => void }) {
  const rootRef = useRef<HTMLDivElement>(null);
  const [stage, setStage] = useState<Stage>();
  const scene = useMemo(() => stage && sceneLayout(stage), [stage]);

  useLayoutEffect(() => {
    const root = rootRef.current;
    const composer = root?.parentElement?.querySelector(
      "[data-session-composer]",
    );
    if (!root) return;
    const measure = () => {
      const pane = root.getBoundingClientRect();
      const box = composer?.getBoundingClientRect();
      const { top, height } = opusStage(
        pane.height,
        box && { top: box.top - pane.top, bottom: box.bottom - pane.top },
      );
      const next = {
        top: Math.round(top),
        width: Math.round(pane.width),
        height: Math.round(height),
      };
      setStage((current) =>
        current &&
        current.top === next.top &&
        current.width === next.width &&
        current.height === next.height
          ? current
          : next,
      );
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(root);
    if (composer) observer.observe(composer);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)");
    if (reducedMotion.matches) {
      onDone();
      return;
    }
    const onMotionChange = () => {
      if (reducedMotion.matches) onDone();
    };
    const timer = window.setTimeout(onDone, DURATION_MS);
    reducedMotion.addEventListener("change", onMotionChange);
    return () => {
      window.clearTimeout(timer);
      reducedMotion.removeEventListener("change", onMotionChange);
    };
  }, [onDone]);

  return (
    <div
      ref={rootRef}
      className="opus-welcome"
      aria-hidden="true"
      style={{ "--opus-duration": `${DURATION_MS}ms` } as CSSProperties}
    >
      <div className="opus-welcome-glow" />
      {stage && scene && stage.height > 0 ? (
        <div
          className="opus-stage"
          style={{ top: stage.top, height: stage.height }}
        >
          {MOTES.map((style, index) => (
            <span
              className={`opus-mote${index % 5 === 0 ? " opus-mote-ring" : ""}`}
              style={style}
              key={index}
            />
          ))}
          <div className="opus-staff">
            {scene.lines.map((path, index) => (
              <svg
                className="opus-staff-line"
                style={
                  {
                    "--line-delay": `${0.15 + index * 0.09}s`,
                  } as CSSProperties
                }
                key={index}
              >
                <path d={path} />
              </svg>
            ))}
            {scene.notes.map(({ style, stemDown }, index) => (
              <span
                className={`opus-note${stemDown ? " opus-note-stem-down" : ""}`}
                style={style}
                key={index}
              >
                <span className="opus-note-head" />
              </span>
            ))}
          </div>
        </div>
      ) : null}
    </div>
  );
}
