import type { CSSProperties, ReactNode } from "react";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { PixelMascot } from "@/features/mascots/PixelMascot";

const renders = vi.hoisted(() => ({ rects: 0, wrappers: 0 }));
vi.mock("react-native", () => ({
  StyleSheet: { absoluteFill: { position: "absolute" }, create: <T,>(s: T) => s },
  View: ({ children, style }: { children?: ReactNode; style?: CSSProperties }) => {
    if (style && "backgroundColor" in style) renders.rects++;
    else renders.wrappers++;
    return <div style={style}>{children}</div>;
  },
}));
vi.mock("@/ui/theme", () => ({
  withAlpha: (color: string) => color,
  themedStyles: () => () => ({}),
}));

(
  globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean }
).IS_REACT_ACT_ENVIRONMENT = true;

let root: ReturnType<typeof createRoot>;
let container: HTMLDivElement;
beforeEach(() => {
  vi.useFakeTimers();
  renders.rects = 0;
  renders.wrappers = 0;
  container = document.createElement("div");
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  vi.useRealTimers();
});

it("flips a working mascot's frame without rebuilding its pixel Views", async () => {
  await act(async () =>
    root.render(<PixelMascot name="ghost" color="#4c9ef5" status="working" />),
  );
  const rects = renders.rects;
  // Both frames are mounted once.
  expect(rects).toBeGreaterThan(0);
  renders.rects = 0;
  await act(async () => vi.advanceTimersByTime(300));
  await act(async () => vi.advanceTimersByTime(300));
  await act(async () => vi.advanceTimersByTime(300));
  expect(renders.rects).toBe(0);
});

it("shows one frame at a time", async () => {
  await act(async () =>
    root.render(<PixelMascot name="ghost" color="#4c9ef5" status="working" />),
  );
  const hidden = () =>
    [...container.querySelectorAll<HTMLElement>("div")].filter(
      (el) => el.style.display === "none",
    ).length;
  expect(hidden()).toBe(1);
  await act(async () => vi.advanceTimersByTime(300));
  expect(hidden()).toBe(1);
});

it("draws a single frame, with no clock, when not working", async () => {
  await act(async () =>
    root.render(<PixelMascot name="ghost" color="#4c9ef5" status="idle" />),
  );
  expect(vi.getTimerCount()).toBe(0);
  expect(
    [...container.querySelectorAll<HTMLElement>("div")].some(
      (el) => el.style.display === "none",
    ),
  ).toBe(false);
});
