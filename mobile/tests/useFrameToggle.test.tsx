import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { useFrameToggle } from "@/features/mascots/useFrameToggle";
import { AnimationsPaused } from "@/ui/AnimationsPaused";

(
  globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean }
).IS_REACT_ACT_ENVIRONMENT = true;

let root: ReturnType<typeof createRoot>;
const frames: boolean[] = [];
function Mascot({ active }: { active: boolean }) {
  frames.push(useFrameToggle(active));
  return null;
}

beforeEach(() => {
  vi.useFakeTimers();
  frames.length = 0;
  root = createRoot(document.createElement("div"));
});
afterEach(() => {
  act(() => root.unmount());
  vi.useRealTimers();
});

it("flips working mascots together on one clock", async () => {
  await act(async () =>
    root.render(
      <>
        <Mascot active />
        <Mascot active />
      </>,
    ),
  );
  frames.length = 0;
  await act(async () => vi.advanceTimersByTime(300));
  // One flip, one render each: both talking.
  expect(frames).toEqual([true, true]);
  expect(vi.getTimerCount()).toBe(1);
});

it("holds a paused mascot at rest with no clock running", async () => {
  await act(async () =>
    root.render(
      <AnimationsPaused value>
        <Mascot active />
      </AnimationsPaused>,
    ),
  );
  await act(async () => vi.advanceTimersByTime(900));
  expect(frames.every((talking) => !talking)).toBe(true);
  expect(vi.getTimerCount()).toBe(0);
});
