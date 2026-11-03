import { expect, it } from "vitest";
import { emptyPressure, samplePressure } from "./resourcePressure";
const hot = { cpuPercent: 92, memoryPressure: 1, stallShare: 0 };
const cool = { cpuPercent: 30, memoryPressure: 1, stallShare: 0 };
it("ignores transient spikes and uses recovery hysteresis and a cooldown", () => {
  let state = samplePressure(emptyPressure(), hot, 0);
  expect(state.active).toBe(false);
  state = samplePressure(state, cool, 3_000);
  for (let i = 0; i < 3; i++)
    state = samplePressure(state, hot, 6_000 + i * 3_000);
  expect(state.active).toBe(true);
  for (let i = 0; i < 5; i++)
    state = samplePressure(state, cool, 15_000 + i * 3_000);
  expect(state.active).toBe(true);
  state = samplePressure(state, cool, 30_000);
  expect(state.active).toBe(false);
  for (let i = 0; i < 4; i++)
    state = samplePressure(state, hot, 33_000 + i * 3_000);
  expect(state.active).toBe(false);
  expect(samplePressure(state, hot, 75_000).active).toBe(true);
});
it("detects sustained memory pressure and stalls without CPU saturation", () => {
  for (const sample of [
    { ...cool, memoryPressure: 2 },
    { ...cool, stallShare: 0.2 },
  ]) {
    let state = emptyPressure();
    for (let i = 0; i < 3; i++)
      state = samplePressure(state, sample, i * 3_000);
    expect(state.active).toBe(true);
  }
});
