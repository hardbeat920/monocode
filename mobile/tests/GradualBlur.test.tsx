import type { ReactNode } from "react";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { GradualBlur } from "@/ui/GradualBlur";

const counts = vi.hoisted(() => ({ masks: 0, blurs: 0, tints: [] as string[] }));
vi.mock("@react-native-masked-view/masked-view", () => ({
  default: ({ children }: { children?: ReactNode }) => {
    counts.masks++;
    return <div>{children}</div>;
  },
}));
vi.mock("expo-blur", () => ({
  BlurView: ({ style }: { style: { backgroundColor?: string }[] }) => {
    counts.blurs++;
    counts.tints.push(style[1].backgroundColor ?? "");
    return null;
  },
}));
vi.mock("expo-linear-gradient", () => ({ LinearGradient: () => null }));
vi.mock("react-native", () => ({
  StyleSheet: { absoluteFill: {}, create: <T,>(styles: T) => styles },
  View: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
}));
vi.mock("@/ui/theme", () => ({
  useScheme: () => "dark",
  withAlpha: (color: string, alpha: number) => `${color}:${alpha}`,
}));

(
  globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean }
).IS_REACT_ACT_ENVIRONMENT = true;

let root: ReturnType<typeof createRoot>;
beforeEach(() => {
  counts.masks = 0;
  counts.blurs = 0;
  counts.tints = [];
  root = createRoot(document.createElement("div"));
});
afterEach(() => act(() => root.unmount()));

const strip = (props: { height?: number; color?: string } = {}) => (
  <GradualBlur edge="bottom" height={props.height ?? 100} color={props.color ?? "#000000"} />
);

it("stacks six live blur layers by default, as it always looked", async () => {
  await act(async () => root.render(strip()));
  expect(counts.masks).toBe(6);
  expect(counts.blurs).toBe(6);
});

it("tints each layer with the original 50% of the background", async () => {
  await act(async () => root.render(strip({ color: "#123456" })));
  expect(counts.tints).toEqual(Array(6).fill("#123456:0.5"));
});

it("does not render again when its parent does", async () => {
  function Parent({ tick }: { tick: number }) {
    return (
      <div data-tick={tick}>
        <GradualBlur edge="top" height={120} color="#101010" />
      </div>
    );
  }
  await act(async () => root.render(<Parent tick={0} />));
  const first = { ...counts };
  for (let tick = 1; tick <= 20; tick++) await act(async () => root.render(<Parent tick={tick} />));
  // Twenty parent renders add no layer renders at all.
  expect(counts.masks).toBe(first.masks);
  expect(counts.blurs).toBe(first.blurs);
});

it("keeps its six layers untouched while only its height changes", async () => {
  await act(async () => root.render(strip({ height: 100 })));
  const first = counts.masks;
  for (let height = 101; height <= 160; height++)
    await act(async () => root.render(strip({ height })));
  // Sixty height changes, as a sliding keyboard makes, rebuild no layer.
  expect(counts.masks).toBe(first);
});

it("rebuilds the layers once when the background changes", async () => {
  await act(async () => root.render(strip({ color: "#000000" })));
  await act(async () => root.render(strip({ color: "#ffffff" })));
  expect(counts.masks).toBe(12);
});
