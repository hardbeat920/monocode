export type ScheduledFlush = { kind: "raf" | "timeout"; id: number };

export function cancelScheduledFlush(handle: ScheduledFlush | null) {
  if (!handle) return;
  if (handle.kind === "raf") cancelAnimationFrame(handle.id);
  else clearTimeout(handle.id);
}

/** Hidden output must keep advancing without driving the whole UI at 60–120Hz. */
export function scheduleHarnessFlush(
  run: () => void,
  foreground: boolean,
): ScheduledFlush {
  if (document.hidden || !foreground) {
    return { kind: "timeout", id: window.setTimeout(run, 100) };
  }
  return { kind: "raf", id: requestAnimationFrame(run) };
}
