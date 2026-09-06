import { useEffect, type CSSProperties } from "react";
import "./AstraWelcome.css";

const DURATION_MS = 7600;
const STARS = Array.from(
  { length: 38 },
  (_, index) =>
    ({
      "--star-x": `${8 + ((index * 37) % 125)}%`,
      "--star-y": `${-12 + ((index * 19) % 48)}%`,
      "--star-delay": `${0.35 + index * 0.12}s`,
      "--star-duration": `${1.4 + (index % 5) * 0.22}s`,
      "--star-length": `${index % 9 === 0 ? 190 : 45 + ((index * 23) % 95)}px`,
      "--star-width": index % 9 === 0 ? "2px" : "1px",
      "--star-brightness": index % 3 === 0 ? 0.95 : 0.5,
    }) as CSSProperties,
);
const SPARKLES = Array.from(
  { length: 36 },
  (_, index) =>
    ({
      "--spark-x": `${3 + ((index * 37) % 94)}%`,
      "--spark-y": `${4 + ((index * 23) % 86)}%`,
      "--spark-size": index % 6 === 0 ? "9px" : `${1 + (index % 3)}px`,
      "--spark-delay": `${0.2 + ((index * 7) % 24) * 0.12}s`,
      "--spark-duration": `${2.2 + (index % 4) * 0.45}s`,
    }) as CSSProperties,
);

/** A decorative layer confined to the session, with no input interception. */
export function AstraWelcome({ onDone }: { onDone: () => void }) {
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
      className="astra-welcome"
      aria-hidden="true"
      style={{ "--astra-duration": `${DURATION_MS}ms` } as CSSProperties}
    >
      <div className="astra-welcome-glow" />
      <div className="astra-solar-system">
        <div className="astra-solar-corona" />
        <div className="astra-solar-wave" />
        <div className="astra-solar-wave astra-solar-wave-late" />
        <div className="astra-solar-orbit">
          <span />
        </div>
        <div className="astra-solar-orbit astra-solar-orbit-outer">
          <span />
        </div>
        <div className="astra-solar-core" />
      </div>
      {SPARKLES.map((style, index) => (
        <span
          className={`astra-sparkle${index % 6 === 0 ? " astra-sparkle-cross" : ""}`}
          style={style}
          key={index}
        />
      ))}
      {STARS.map((style, index) => (
        <div className="astra-star-flight" style={style} key={index}>
          <span className="astra-star" />
        </div>
      ))}
    </div>
  );
}
