import type { ReactNode } from "react";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { Shimmer } from "@/ui/Shimmer";

const loops = vi.hoisted(() => ({ started: 0, cancelled: 0, running: true }));
vi.mock("@/ui/useAnimationsRunning", () => ({
  useAnimationsRunning: () => loops.running,
}));
vi.mock("@react-native-masked-view/masked-view", () => ({
  default: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
}));
vi.mock("expo-linear-gradient", () => ({ LinearGradient: () => null }));
vi.mock("react-native", () => ({
  StyleSheet: {
    absoluteFill: {},
    create: <T,>(styles: T) => styles,
    flatten: (style: object) => style,
  },
  Text: ({ children }: { children?: ReactNode }) => <span>{children}</span>,
  View: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
}));
vi.mock("react-native-reanimated", () => ({
  default: { View: ({ children }: { children?: ReactNode }) => <div>{children}</div> },
  Easing: { linear: 0 },
  useSharedValue: () => ({ get: () => 0, set: () => {} }),
  useAnimatedStyle: () => ({}),
  withTiming: () => 0,
  withRepeat: () => {
    loops.started++;
    return 0;
  },
  cancelAnimation: () => {
    loops.cancelled++;
  },
}));
vi.mock("@/ui/theme", () => ({
  useColors: () => ({ content: "#ffffff" }),
  withAlpha: (color: string) => color,
}));

(
  globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean }
).IS_REACT_ACT_ENVIRONMENT = true;

let root: ReturnType<typeof createRoot>;
beforeEach(() => {
  loops.started = 0;
  loops.cancelled = 0;
  loops.running = true;
  root = createRoot(document.createElement("div"));
});
afterEach(() => act(() => root.unmount()));

const shimmer = <Shimmer style={{ color: "#fff" }}>Working</Shimmer>;

it("sweeps while animations run", async () => {
  await act(async () => root.render(shimmer));
  expect(loops.started).toBe(1);
});

it("never starts its loop while paused, backgrounded or reduced", async () => {
  loops.running = false;
  await act(async () => root.render(shimmer));
  expect(loops.started).toBe(0);
});

it("cancels the loop when animations pause, and restarts on resume", async () => {
  await act(async () => root.render(shimmer));
  loops.running = false;
  await act(async () => root.render(<Shimmer style={{ color: "#fff" }}>Working</Shimmer>));
  expect(loops.cancelled).toBe(1);
  expect(loops.started).toBe(1);
  loops.running = true;
  await act(async () => root.render(shimmer));
  expect(loops.started).toBe(2);
});
