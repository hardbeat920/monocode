import { act, createRef, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { GlassBlurTarget, LiquidView } from "@/ui/LiquidView";
import type { View } from "react-native";

const runtime = vi.hoisted(() => ({
  os: "android",
  nativeGlass: false,
  glass: {} as { isInteractive?: boolean; style?: unknown[] },
  blur: {} as {
    blurTarget?: unknown;
    blurMethod?: string;
    intensity?: number;
    style?: unknown[];
  },
}));
vi.mock("expo-blur", () => ({
  BlurView: (props: typeof runtime.blur & { children?: ReactNode }) => {
    runtime.blur = props;
    return <div>{props.children}</div>;
  },
}));
vi.mock("expo-glass-effect", () => ({
  isLiquidGlassAvailable: () => runtime.nativeGlass,
  GlassView: (props: typeof runtime.glass) => {
    runtime.glass = props;
    return null;
  },
}));
vi.mock("expo-haptics", () => ({ impactAsync: vi.fn() }));
vi.mock("expo-linear-gradient", () => ({
  LinearGradient: ({ children }: { children?: ReactNode }) => (
    <div>{children}</div>
  ),
}));
vi.mock("react-native", () => ({
  Platform: {
    get OS() {
      return runtime.os;
    },
  },
  StyleSheet: { hairlineWidth: 1 },
  Pressable: () => null,
}));
vi.mock("react-native-reanimated", () => ({
  default: {
    View: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
  },
  Easing: { out: vi.fn(), cubic: vi.fn() },
  useSharedValue: (value: number) => ({ get: () => value }),
  useAnimatedStyle: () => ({}),
}));
vi.mock("@/ui/theme", () => ({
  useScheme: () => "dark",
  withAlpha: (color: string, alpha: number) => `${color}:${alpha}`,
  themedStyles: (fn: (colors: { surface: string }) => object) => () =>
    fn({ surface: "surface" }),
}));
let root: ReturnType<typeof createRoot>;
beforeEach(() => {
  (
    globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }
  ).IS_REACT_ACT_ENVIRONMENT = true;
  runtime.os = "android";
  runtime.nativeGlass = false;
  root = createRoot(document.createElement("div"));
});
afterEach(() => act(() => root.unmount()));
it("enables Android blur against an explicit target and lightens the surface overlay", () => {
  const target = createRef<View>();
  act(() =>
    root.render(
      <GlassBlurTarget value={target}>
        <LiquidView>
          <span>Input</span>
        </LiquidView>
      </GlassBlurTarget>,
    ),
  );
  expect(runtime.blur.blurTarget).toBe(target);
  expect(runtime.blur.blurMethod).toBe("dimezisBlurViewSdk31Plus");
  expect(runtime.blur.intensity).toBe(30);
  expect(runtime.blur.style).toContainEqual({
    backgroundColor: "surface:0.45",
  });
});
it("preserves the untargeted fallback and iOS blur settings", () => {
  act(() =>
    root.render(
      <LiquidView>
        <span>Input</span>
      </LiquidView>,
    ),
  );
  expect(runtime.blur.blurMethod).toBeUndefined();
  expect(runtime.blur.style).not.toContainEqual({
    backgroundColor: "surface:0.45",
  });
  runtime.os = "ios";
  act(() =>
    root.render(
      <GlassBlurTarget value={createRef<View>()}>
        <LiquidView>
          <span>Input</span>
        </LiquidView>
      </GlassBlurTarget>,
    ),
  );
  expect(runtime.blur.blurMethod).toBeUndefined();
  expect(runtime.blur.blurTarget).toBeUndefined();
});

const clipped = (style: unknown[] = []) =>
  style.flat().some((part) => (part as { overflow?: string } | null)?.overflow === "hidden");

it("leaves interactive native glass unclipped so it can stretch past its frame", () => {
  runtime.os = "ios";
  runtime.nativeGlass = true;
  act(() => root.render(<LiquidView interactive><span>Search</span></LiquidView>));
  expect(runtime.glass.isInteractive).toBe(true);
  expect(clipped(runtime.glass.style)).toBe(false);
});

it("keeps still native glass clipped to its frame", () => {
  runtime.os = "ios";
  runtime.nativeGlass = true;
  act(() => root.render(<LiquidView><span>Title</span></LiquidView>));
  expect(runtime.glass.isInteractive).toBe(false);
  expect(clipped(runtime.glass.style)).toBe(true);
});
