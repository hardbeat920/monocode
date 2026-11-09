import { Profiler, type ReactNode } from "react";

/** Renders slower than this are logged. */
const SLOW_MS = 8;

/** Off unless asked for: start Metro with EXPO_PUBLIC_RENDER_TIMING=1. */
const ENABLED = __DEV__ && process.env.EXPO_PUBLIC_RENDER_TIMING === "1";

/**
 * Dev only, when enabled: logs how long `children` took to render when slow,
 * as `[render] id phase ms` in the Metro log, to find what makes a screen slow
 * to arrive. Otherwise renders the children alone.
 */
export function Timed({ id, children }: { id: string; children: ReactNode }) {
  if (!ENABLED) return children;
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
