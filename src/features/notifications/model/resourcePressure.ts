export type ResourceSample = {
  cpuPercent: number | null;
  memoryPressure: number | null;
  stallShare: number;
};
export type PressureState = {
  active: boolean;
  high: number;
  low: number;
  lastNotice: number;
  reasons: string[];
};
export const emptyPressure = (): PressureState => ({
  active: false,
  high: 0,
  low: 0,
  lastNotice: -Infinity,
  reasons: [],
});

/** Three high samples to enter, six healthy samples to recover, 60s cooldown. */
export function samplePressure(
  previous: PressureState,
  sample: ResourceSample,
  now: number,
): PressureState {
  const reasons = [
    (sample.cpuPercent ?? 0) >= 85 ? "CPU saturation" : "",
    (sample.memoryPressure ?? 0) >= 2 ? "Memory pressure" : "",
    sample.stallShare >= 0.1 ? "UI stalls" : "",
  ].filter(Boolean);
  const healthy =
    (sample.cpuPercent ?? 0) < 65 &&
    (sample.memoryPressure ?? 0) < 2 &&
    sample.stallShare < 0.03;
  const high = reasons.length ? previous.high + 1 : 0;
  const low = healthy ? previous.low + 1 : 0;
  if (!previous.active && high >= 3 && now - previous.lastNotice >= 60_000)
    return { active: true, high, low, lastNotice: now, reasons };
  if (previous.active && low >= 6)
    return { ...previous, active: false, high: 0, low, reasons: [] };
  return {
    ...previous,
    high,
    low,
    reasons: previous.active && reasons.length ? reasons : previous.reasons,
  };
}
