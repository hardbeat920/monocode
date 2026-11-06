import type { ReactNode } from "react";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { TerminalSpinner } from "@/features/sessions/TerminalSpinner";

const seen = vi.hoisted(() => ({
  renders: 0,
  running: true,
  styles: [] as (() => { transform: { translateY: number }[] })[],
  layout: null as null | ((event: { nativeEvent: { layout: { height: number } } }) => void),
}));

vi.mock("react-native", () => ({
  StyleSheet: { create: <T,>(s: T) => s },
  View: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
  Text: ({
    children,
    onLayout,
  }: {
    children?: ReactNode;
    onLayout?: typeof seen.layout;
  }) => {
    seen.renders += 1;
    if (onLayout) seen.layout = onLayout;
    return <span>{children}</span>;
  },
}));
vi.mock("react-native-reanimated", () => {
  const mutable = (initial: number) => {
    let value = initial;
    return {
      get: () => value,
      set: (next: number) => {
        value = next;
      },
    };
  };
  return {
    default: { View: ({ children }: { children?: ReactNode }) => <div>{children}</div> },
    makeMutable: mutable,
    useSharedValue: mutable,
    useAnimatedStyle: (style: (typeof seen.styles)[number]) => {
      seen.styles.push(style);
      return style();
    },
  };
});
vi.mock("@/ui/useAnimationsRunning", () => ({
  useAnimationsRunning: () => seen.running,
}));

(
  globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean }
).IS_REACT_ACT_ENVIRONMENT = true;

let root: ReturnType<typeof createRoot>;
let container: HTMLDivElement;
beforeEach(() => {
  vi.useFakeTimers();
  seen.renders = 0;
  seen.running = true;
  seen.styles.length = 0;
  seen.layout = null;
  container = document.createElement("div");
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  vi.useRealTimers();
});

const offset = () => seen.styles.at(-1)?.().transform[0].translateY ?? 0;

it("steps a line at a time without rendering", async () => {
  await act(async () => root.render(<TerminalSpinner />));
  seen.layout?.({ nativeEvent: { layout: { height: 160 } } });
  seen.renders = 0;
  const start = offset();
  await act(async () => vi.advanceTimersByTime(80));
  expect(offset()).toBe(start - 16);
  await act(async () => vi.advanceTimersByTime(800));
  // A render here would commit the tree twelve times a second.
  expect(seen.renders).toBe(0);
});

it("shares one clock between spinners", async () => {
  await act(async () =>
    root.render(
      <>
        <TerminalSpinner />
        <TerminalSpinner />
      </>,
    ),
  );
  expect(vi.getTimerCount()).toBe(1);
});

it("holds still with no clock where animations are held", async () => {
  seen.running = false;
  await act(async () => root.render(<TerminalSpinner />));
  seen.layout?.({ nativeEvent: { layout: { height: 160 } } });
  expect(vi.getTimerCount()).toBe(0);
  expect(offset()).toBeCloseTo(0);
});

it("has every frame on its strip, behind a window sized to one", async () => {
  await act(async () => root.render(<TerminalSpinner />));
  const [sizer, strip] = container.querySelectorAll("span");
  expect(sizer.textContent).toBe("⠋");
  expect(strip.textContent?.split("\n")).toHaveLength(10);
});
