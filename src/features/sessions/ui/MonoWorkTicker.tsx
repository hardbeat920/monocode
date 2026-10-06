import { useEffect, useRef, useState } from "react";
import {
  Bot,
  CircleDashed,
  Minus,
  PenLine,
  Search,
  Terminal,
  Wrench,
} from "../../../shared/ui/icons";
import { Shimmer } from "../../../shared/ui/Shimmer";
import type { MonoWorkStatus } from "../model/monoWorkStatus";

const TICK_MS = 340;
const HOLD_MS = 850;

/** One fixed-height line: the new status pushes the previous one upward. */
export function MonoWorkTicker({ status }: { status: MonoWorkStatus }) {
  const [frame, setFrame] = useState(() => ({
    current: status,
    previous: null as MonoWorkStatus | null,
    sequence: 0,
  }));
  const lastTick = useRef(performance.now());
  useEffect(() => {
    const current = frame.current;
    if (
      current.key === status.key &&
      current.label === status.label &&
      current.kind === status.kind &&
      current.active === status.active
    )
      return;
    const reduced = window.matchMedia?.(
      "(prefers-reduced-motion: reduce)",
    ).matches;
    const elapsed = performance.now() - lastTick.current;
    // Bursts coalesce into the latest activity, without interrupting a tick.
    const delay = reduced
      ? 0
      : Math.max(
          0,
          Math.max(frame.previous ? TICK_MS : 0, status.active ? HOLD_MS : 0) -
            elapsed,
        );
    const tick = () => {
      lastTick.current = performance.now();
      setFrame((previous) => ({
        current: status,
        previous: reduced ? null : previous.current,
        sequence: previous.sequence + 1,
      }));
    };
    if (!delay) {
      tick();
      return;
    }
    const timer = window.setTimeout(tick, delay);
    return () => window.clearTimeout(timer);
  }, [status.key, status.label, status.kind, status.active, frame]);

  useEffect(() => {
    if (!frame.previous) return;
    const sequence = frame.sequence;
    const timer = window.setTimeout(() => {
      setFrame((current) =>
        current.sequence === sequence
          ? { ...current, previous: null }
          : current,
      );
    }, TICK_MS);
    return () => window.clearTimeout(timer);
  }, [frame.sequence, frame.previous]);

  return (
    <div className="mono-work-ticker" role="status">
      <span className="sr-only">{frame.current.label}</span>
      <div
        key={frame.sequence}
        className="mono-work-ticker-track"
        data-moving={!!frame.previous}
        aria-hidden="true"
      >
        {frame.previous ? <StatusRow status={frame.previous} /> : null}
        <StatusRow status={frame.current} />
      </div>
    </div>
  );
}

function StatusRow({ status }: { status: MonoWorkStatus }) {
  const Icon = {
    run: Terminal,
    edit: PenLine,
    research: Search,
    agent: Bot,
    other: Wrench,
    think: CircleDashed,
    note: Minus,
  }[status.kind];
  return (
    <div className="mono-work-ticker-row flex min-w-0 items-center gap-1.5">
      <Icon
        className="size-3.5 shrink-0 text-content/45 transition-colors duration-200 group-hover/mono-work:text-content/80"
        strokeWidth={1.75}
      />
      {status.active ? (
        <Shimmer className="min-w-0 truncate font-sans text-sm" duration={1.6}>
          {status.label}
        </Shimmer>
      ) : (
        <span className="min-w-0 truncate font-sans text-sm text-content/50 transition-colors duration-200 group-hover/mono-work:text-content/80">
          {status.label}
        </span>
      )}
    </div>
  );
}
