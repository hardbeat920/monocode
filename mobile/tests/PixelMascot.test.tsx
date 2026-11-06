import type { CSSProperties, ReactNode } from "react";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { PixelMascot } from "@/features/mascots/PixelMascot";

const renders = vi.hoisted(() => ({ rects: 0, wrappers: 0 }));
const motion = vi.hoisted(() => ({ running: true, writes: [] as number[] }));

type ViewProps = {
  children?: ReactNode;
  style?: CSSProperties | CSSProperties[];
  collapsable?: boolean;
};
function tally(style: CSSProperties | undefined) {
  if (style && "backgroundColor" in style) renders.rects++;
  else renders.wrappers++;
}
function MockView({ children, style, collapsable }: ViewProps) {
  const flat = Array.isArray(style) ? Object.assign({}, ...style) : style;
  tally(flat);
  return (
    <div style={flat} data-kept={collapsable === false ? "" : undefined}>
      {children}
    </div>
  );
}
vi.mock("react-native", () => ({
  StyleSheet: { absoluteFill: { position: "absolute" }, create: <T,>(s: T) => s },
  View: MockView,
}));
vi.mock("react-native-reanimated", () => ({
  default: { View: MockView },
  makeMutable: (initial: number) => {
    let value = initial;
    return {
      get: () => value,
      set: (next: number) => {
        value = next;
        motion.writes.push(next);
      },
    };
  },
  useAnimatedStyle: <T,>(style: () => T) => style(),
}));
vi.mock("@/ui/useAnimationsRunning", () => ({
  useAnimationsRunning: () => motion.running,
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
  motion.running = true;
  motion.writes.length = 0;
  container = document.createElement("div");
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  vi.useRealTimers();
});

it("flips a working mascot's frame without rendering at all", async () => {
  await act(async () =>
    root.render(<PixelMascot name="ghost" color="#4c9ef5" status="working" />),
  );
  // Both frames are mounted once.
  expect(renders.rects).toBeGreaterThan(0);
  renders.rects = 0;
  renders.wrappers = 0;
  await act(async () => vi.advanceTimersByTime(900));
  // A render here would commit the tree three times a second.
  expect(renders).toEqual({ rects: 0, wrappers: 0 });
  expect(motion.writes).toHaveLength(3);
});

it("shows one frame at a time", async () => {
  await act(async () =>
    root.render(<PixelMascot name="ghost" color="#4c9ef5" status="working" />),
  );
  const hidden = [...container.querySelectorAll<HTMLElement>("[data-kept]")].filter(
    (el) => el.style.opacity === "0",
  );
  expect(hidden).toHaveLength(1);
});

it("keeps both frames' native views mounted", async () => {
  await act(async () =>
    root.render(<PixelMascot name="ghost" color="#4c9ef5" status="working" />),
  );
  const frames = [...container.querySelectorAll<HTMLElement>("[data-kept]")];
  expect(frames).toHaveLength(2);
  // Fabric drops a `display: none` view's native subtree on every flip.
  expect(frames.every((frame) => frame.style.display !== "none")).toBe(true);
});

it("holds a working mascot at rest, with no clock, where animations are held", async () => {
  motion.running = false;
  await act(async () =>
    root.render(<PixelMascot name="ghost" color="#4c9ef5" status="working" />),
  );
  expect(vi.getTimerCount()).toBe(0);
  const [rest, talk] = container.querySelectorAll<HTMLElement>("[data-kept]");
  expect(rest.style.opacity).toBe("1");
  expect(talk.style.opacity).toBe("0");
});

it("draws a single frame, with no clock, when not working", async () => {
  await act(async () =>
    root.render(<PixelMascot name="ghost" color="#4c9ef5" status="idle" />),
  );
  expect(vi.getTimerCount()).toBe(0);
  expect(
    [...container.querySelectorAll<HTMLElement>("div")].some(
      (el) => el.style.opacity === "0",
    ),
  ).toBe(false);
});
