import { Profiler, type ReactNode } from "react";

/** Renders slower than this are logged. */
const SLOW_MS = 8;

/**
 * Dev only: logs how long `children` took to render when slow, as
 * `[render] id phase ms` in the Metro log, to find what makes a screen slow
 * to arrive. Renders the children alone in a release build.
 */
export function Timed({ id, children }: { id: string; children: ReactNode }) {
  if (!__DEV__) return children;
  return (
    <Profiler
      id={id}
      onRender={(name, phase, actual) => {
        if (actual > SLOW_MS) console.log(`[render] ${name} ${phase} ${Math.round(actual)}ms`);
      }}
    >
      {children}
    </Profiler>
  );
}
