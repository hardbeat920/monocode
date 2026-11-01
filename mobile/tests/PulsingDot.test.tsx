import type { ReactNode } from "react";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

const env = vi.hoisted(() => ({ running: true, started: 0, cancelled: 0 }));
vi.mock("react-native", () => ({
  Text: ({ children }: { children?: ReactNode }) => <span>{children}</span>,
  View: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
}));
vi.mock("react-native-reanimated", () => ({
  default: { View: ({ children }: { children?: ReactNode }) => <div>{children}</div> },
  Easing: { out: () => () => 0, cubic: () => 0 },
  useAnimatedStyle: () => ({}),
  useSharedValue: (value: number) => {
    let current = value;
    return { get: () => current, set: (next: number) => void (current = next) };
  },
  withRepeat: (animation: unknown) => {
    env.started++;
    return animation;
  },
  withTiming: (to: number) => to,
  cancelAnimation: () => void env.cancelled++,
}));
vi.mock("@/ui/useAnimationsRunning", () => ({ useAnimationsRunning: () => env.running }));
vi.mock("@/features/sessions/SessionScreen", () => ({ useSessionLayout: () => ({}) }));
vi.mock("@/ui/LiquidView", () => ({ LiquidView: ({ children }: { children?: ReactNode }) => <div>{children}</div> }));
vi.mock("@/ui/theme", () => ({
  themedStyles: () => () => new Proxy({}, { get: () => ({}) }),
  useColors: () => ({}),
  withAlpha: () => "",
}));
vi.mock("@/features/connection/ConnectionProvider", () => ({ useConnection: () => ({}) }));

import { PulsingDot } from "@/features/connection/ConnectionNotice";

(
  globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean }
).IS_REACT_ACT_ENVIRONMENT = true;

let root: ReturnType<typeof createRoot>;
beforeEach(() => {
  env.running = true;
  env.started = 0;
  env.cancelled = 0;
  root = createRoot(document.createElement("div"));
});
afterEach(() => act(() => root.unmount()));

it("pulses while animations run, and stops when they are paused", async () => {
  await act(async () => root.render(<PulsingDot />));
  expect(env.started).toBe(1);
  env.running = false;
  await act(async () => root.render(<PulsingDot />));
  expect(env.cancelled).toBe(1);
  expect(env.started).toBe(1);
  env.running = true;
  await act(async () => root.render(<PulsingDot />));
  expect(env.started).toBe(2);
});

it("never starts the loop when it mounts paused", async () => {
  env.running = false;
  await act(async () => root.render(<PulsingDot />));
  expect(env.started).toBe(0);
});
