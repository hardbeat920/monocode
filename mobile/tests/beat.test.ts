import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { createBeat } from "@/ui/beat";

vi.mock("react-native-reanimated", () => ({
  makeMutable: (initial: number) => {
    let value = initial;
    return {
      get: () => value,
      set: (next: number) => {
        value = next;
      },
    };
  },
}));
vi.mock("@/ui/useAnimationsRunning", () => ({ useAnimationsRunning: () => true }));

beforeEach(() => {
  vi.useFakeTimers();
});
afterEach(() => {
  vi.useRealTimers();
});

it("counts round its steps, on one clock however many hold it", () => {
  const beat = createBeat(80, 3);
  const first = beat.hold();
  const second = beat.hold();
  expect(vi.getTimerCount()).toBe(1);
  const seen: number[] = [];
  for (let i = 0; i < 4; i += 1) {
    vi.advanceTimersByTime(80);
    seen.push(beat.counting().get());
  }
  expect(seen).toEqual([1, 2, 0, 1]);
  first();
  second();
});

it("stops once the last holder lets go, and carries on from there", () => {
  const beat = createBeat(80, 3);
  const first = beat.hold();
  const second = beat.hold();
  vi.advanceTimersByTime(80);
  first();
  expect(vi.getTimerCount()).toBe(1);
  second();
  expect(vi.getTimerCount()).toBe(0);
  vi.advanceTimersByTime(800);
  expect(beat.counting().get()).toBe(1);
  const again = beat.hold();
  vi.advanceTimersByTime(80);
  expect(beat.counting().get()).toBe(2);
  again();
});

it("keeps what's held still at 0 as the count moves", () => {
  const beat = createBeat(80, 3);
  const release = beat.hold();
  vi.advanceTimersByTime(80);
  expect(beat.resting().get()).toBe(0);
  release();
});
