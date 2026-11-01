import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { useWordFade } from "@/features/sessions/wordReveal";

(
  globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean }
).IS_REACT_ACT_ENVIRONMENT = true;

let renders = 0;
const nows: number[] = [];
let fadeMs = 0;
function Reply({ count }: { count: number }) {
  const fade = useWordFade(count, true);
  const { now } = fade;
  fadeMs = fade.fadeMs;
  renders++;
  nows.push(now);
  return null;
}

// One 60 Hz frame per act, so each state update renders as it would on a device.
const frames = async (ms: number) => {
  for (let t = 0; t < ms; t += 16) await act(async () => void (await vi.advanceTimersByTimeAsync(16)));
};
let root: ReturnType<typeof createRoot>;
beforeEach(() => {
  vi.useFakeTimers({ toFake: ["requestAnimationFrame", "cancelAnimationFrame", "performance", "setTimeout", "clearTimeout"] });
  renders = 0;
  nows.length = 0;
  root = createRoot(document.createElement("div"));
});
afterEach(() => {
  act(() => root.unmount());
  vi.useRealTimers();
});

it("steps the fade on every frame, as it always did, and settles", async () => {
  await act(async () => root.render(<Reply count={1} />));
  await act(async () => root.render(<Reply count={5} />));
  renders = 0;
  nows.length = 0;
  // A fade lasts a few hundred ms; two seconds covers it and then some.
  await frames(2000);
  // One step per 16 ms frame while it fades: the full frame rate, unthrottled.
  expect(renders).toBeGreaterThanOrEqual(Math.floor(fadeMs / 16) - 1);
  const gaps = nows.slice(1).map((now, i) => now - nows[i]);
  expect(Math.max(...gaps.slice(0, Math.floor(fadeMs / 16) - 2))).toBeLessThanOrEqual(17);
  // Nothing renders once the newest word has settled.
  const settled = renders;
  await frames(2000);
  expect(renders).toBe(settled);
});
