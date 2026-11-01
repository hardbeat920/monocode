import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

const world = vi.hoisted(() => ({ focused: true }));
vi.mock("react-native", () => ({
  AppState: { currentState: "active", addEventListener: () => ({ remove: () => {} }) },
}));
vi.mock("expo-router", () => ({ useIsFocused: () => world.focused }));

import { PolledResource } from "@/features/connection/polledResource";
import { usePolled } from "@/features/connection/usePolled";

(
  globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean }
).IS_REACT_ACT_ENVIRONMENT = true;

let shown: number | null = null;
function Badge({ resource, fetch }: { resource: PolledResource<number>; fetch: () => Promise<number> }) {
  shown = usePolled(resource, fetch, true).value;
  return null;
}
let root: ReturnType<typeof createRoot>;
beforeEach(() => {
  vi.useFakeTimers();
  world.focused = true;
  shown = null;
  root = createRoot(document.createElement("div"));
});
afterEach(() => {
  act(() => root.unmount());
  vi.useRealTimers();
});

it("a screen that regains focus reads at once, so its badge is not stale", async () => {
  const resource = new PolledResource<number>({ merge: (held, next) => (held === next ? held : next) });
  let value = 1;
  const fetch = vi.fn(async () => value);
  const render = () => act(async () => root.render(<Badge resource={resource} fetch={fetch} />));
  await render();
  await act(async () => void (await vi.advanceTimersByTimeAsync(0)));
  expect(shown).toBe(1);

  // Another screen covers it: no reads while away, though the data changes.
  world.focused = false;
  await render();
  value = 2;
  await act(async () => void (await vi.advanceTimersByTimeAsync(4000)));
  expect(fetch).toHaveBeenCalledTimes(1);
  expect(shown).toBe(1);

  // Back in focus four seconds into the interval: it reads now, not in six seconds.
  world.focused = true;
  await render();
  await act(async () => void (await vi.advanceTimersByTimeAsync(0)));
  expect(fetch).toHaveBeenCalledTimes(2);
  expect(shown).toBe(2);
});
