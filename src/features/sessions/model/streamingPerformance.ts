import { useCallback, useLayoutEffect, useRef } from "react";
import type { HarnessEvent } from "../../../integrations/harness/core/types";
import type { Block } from "./session";

const enabled =
  typeof localStorage !== "undefined" &&
  localStorage.getItem("monocode.measureStreaming") === "1";
const arrivals = new WeakMap<HarnessEvent, number>();
type Sample = {
  received: number;
  applied: number;
  ready?: number;
  length: number;
};
export type StreamMeasurement = { pending: Sample[] };
const measurements = new WeakMap<Block, StreamMeasurement>();

export function noteStreamArrival(event: HarnessEvent, foreground = true) {
  // Time spent deliberately batching an unseen chat is not visible-stream
  // latency. Activation is measured separately by the navigation timing.
  if (enabled && foreground && event.type === "message.delta")
    arrivals.set(event, performance.now());
}

export function noteStreamApplied(
  before: Block[],
  after: Block[],
  events: readonly HarnessEvent[],
  foreground: boolean,
) {
  if (!enabled) return;
  const delta = events.find((event) => arrivals.has(event));
  if (!delta) return;
  let block: Block | undefined;
  for (let index = after.length - 1; index >= 0; index--) {
    if (after[index].role === "assistant") {
      block = after[index];
      break;
    }
  }
  if (!block) return;
  const previous = before.find((entry) => entry.id === block.id);
  const measurement = previous
    ? (measurements.get(previous) ?? { pending: [] })
    : { pending: [] };
  if (!foreground) measurement.pending.length = 0;
  else if (previous?.text !== block.text) {
    measurement.pending.push({
      received: arrivals.get(delta)!,
      applied: performance.now(),
      length: block.text.length,
    });
    // Instrumentation remains bounded even if the renderer is hidden/stalled.
    if (measurement.pending.length > 64) measurement.pending.shift();
  }
  measurements.set(block, measurement);
}

export const streamMeasurement = (block: Block) =>
  enabled ? measurements.get(block) : undefined;

/** Source readiness and the inner Markdown renderer's commit are separate. */
export function useStreamPresentation(
  measurement: StreamMeasurement | undefined,
  sourceLength: number,
  visible: boolean,
) {
  const showing = useRef(visible);
  showing.current = visible;
  useLayoutEffect(
    () => () => {
      showing.current = false;
    },
    [],
  );
  useLayoutEffect(() => {
    if (!measurement) return;
    if (!visible) {
      measurement.pending.length = 0;
      return;
    }
    const now = performance.now();
    for (const sample of measurement.pending) {
      if (sample.length <= sourceLength) sample.ready ??= now;
    }
  }, [measurement, sourceLength, visible]);

  // Call from the rendered blocks' layout effects, after Streamdown's internal
  // transition. The outer component receiving source is not a visible update.
  return useCallback(
    (shownLength: number) => {
      if (!measurement || !visible) return;
      const committed = performance.now();
      const shown = measurement.pending.filter(
        (sample) => sample.length <= shownLength,
      );
      measurement.pending = measurement.pending.filter(
        (sample) => sample.length > shownLength,
      );
      if (!shown.length) return;
      requestAnimationFrame(() =>
        requestAnimationFrame(() => {
          if (!showing.current || document.hidden) return;
          const presented = performance.now();
          if (
            performance.getEntriesByName("monocode:stream-visible").length >=
            300
          )
            performance.clearMeasures("monocode:stream-visible");
          for (const sample of shown)
            performance.measure("monocode:stream-visible", {
              start: sample.received,
              end: presented,
              detail: {
                queueMs: sample.applied - sample.received,
                commitMs: (sample.ready ?? committed) - sample.applied,
                revealMs: committed - (sample.ready ?? committed),
                presentationMs: presented - committed,
              },
            });
        }),
      );
      // Two animation frames bound presentation; they do not expose the exact
      // compositor timestamp. No text or session identifiers are recorded.
    },
    [measurement, visible],
  );
}
