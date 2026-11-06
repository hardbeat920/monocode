import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { AnimationsPaused } from "@/ui/AnimationsPaused";
import { useAnimationsRunning } from "@/ui/useAnimationsRunning";

const env = vi.hoisted(() => ({
  reduced: false,
  state: "active",
  listeners: new Set<() => void>(),
}));
vi.mock("react-native", () => ({
  AppState: {
    get currentState() {
      return env.state;
    },
    addEventListener: (_: string, listener: () => void) => {
      env.listeners.add(listener);
      return { remove: () => env.listeners.delete(listener) };
    },
  },
}));
vi.mock("react-native-reanimated", () => ({
  useReducedMotion: () => env.reduced,
}));

(
  globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean }
).IS_REACT_ACT_ENVIRONMENT = true;

let root: ReturnType<typeof createRoot>;
const seen: boolean[] = [];
function Probe() {
  seen.push(useAnimationsRunning());
  return null;
}
const last = () => seen[seen.length - 1];

beforeEach(() => {
  env.reduced = false;
  env.state = "active";
  env.listeners.clear();
  seen.length = 0;
  root = createRoot(document.createElement("div"));
});
afterEach(() => act(() => root.unmount()));

it("runs when visible, in the foreground, with motion allowed", async () => {
  await act(async () => root.render(<Probe />));
  expect(last()).toBe(true);
});

it("stops under a paused tree", async () => {
  await act(async () =>
    root.render(
      <AnimationsPaused value>
        <Probe />
      </AnimationsPaused>,
    ),
  );
  expect(last()).toBe(false);
});

it("stops with Reduce Motion on", async () => {
  env.reduced = true;
  await act(async () => root.render(<Probe />));
  expect(last()).toBe(false);
});

it("stops when the app goes to the background and resumes after", async () => {
  await act(async () => root.render(<Probe />));
  env.state = "background";
  await act(async () => env.listeners.forEach((notify) => notify()));
  expect(last()).toBe(false);
  env.state = "active";
  await act(async () => env.listeners.forEach((notify) => notify()));
  expect(last()).toBe(true);
});

it("stops while inactive, as under a system alert or the app switcher", async () => {
  await act(async () => root.render(<Probe />));
  env.state = "inactive";
  await act(async () => env.listeners.forEach((notify) => notify()));
  expect(last()).toBe(false);
});

it("drops its app state listener on unmount", async () => {
  await act(async () => root.render(<Probe />));
  expect(env.listeners.size).toBe(1);
  await act(async () => root.unmount());
  expect(env.listeners.size).toBe(0);
  root = createRoot(document.createElement("div"));
});
