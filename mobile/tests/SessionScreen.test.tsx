import { act, StrictMode, useState, type ReactNode } from "react";
import { forwardRef, use } from "react";
import { GlassBlurTarget } from "@/ui/LiquidView";
import { createRoot } from "react-dom/client";
import { describe, expect, it, vi } from "vitest";
import {
  SessionFrame,
  SessionContent,
  SessionScreen,
  useSessionLayout,
} from "@/features/sessions/SessionScreen";
const layout = vi.hoisted(() => ({
  OS: "ios",
  onTargetLayout: undefined as undefined | (() => void),
  onBelowLayout: undefined as undefined | ((event: { nativeEvent: { layout: { height: number } } }) => void),
}));
vi.mock("expo-sqlite/kv-store", () => ({ default: { getItemSync: () => null } }));
vi.mock("expo-blur", () => ({
  BlurTargetView: forwardRef<HTMLDivElement, { children?: ReactNode; onLayout?: () => void }>(({ children, onLayout }, ref) => {
    layout.onTargetLayout = onLayout;
    return <div ref={ref} data-blur-target>{children}</div>;
  }),
}));

vi.mock("react-native", () => {
  const View = ({ children, onLayout, style }: {
    children?: ReactNode;
    onLayout?: typeof layout.onBelowLayout;
    style?: unknown;
  }) => {
    if (onLayout && Array.isArray(style) && style.some((entry) => entry?.minHeight === 22)) {
      layout.onBelowLayout = onLayout;
    }
    return <div>{children}</div>;
  };
  return {
    Platform: { get OS() { return layout.OS; } },
    View,
    Text: View,
    StyleSheet: { create: (styles: unknown) => styles },
    useColorScheme: () => "dark",
  };
});
vi.mock("react-native-keyboard-controller", () => ({
  KeyboardGestureArea: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
  useKeyboardHandler: () => {},
}));
vi.mock("react-native-reanimated", () => ({
  default: { View: ({ children }: { children?: ReactNode }) => <div>{children}</div> },
  Easing: { out: () => {}, cubic: () => {} },
  interpolate: () => 0,
  useAnimatedStyle: () => ({}),
  useSharedValue: (initial: number) =>
    useState(() => {
      let value = initial;
      return { get: () => value, set: (next: number) => { value = next; } };
    })[0],
  withTiming: (value: number) => value,
}));
vi.mock("react-native-safe-area-context", () => ({
  useSafeAreaInsets: () => ({ top: 20, bottom: 0, left: 0, right: 0 }),
}));
vi.mock("@/ui/CircleButton", () => ({ CircleButton: () => null }));
vi.mock("@/ui/GradualBlur", () => ({ GradualBlur: () => null }));
vi.mock("@/ui/Icon", () => ({ Icon: () => null }));
vi.mock("@/ui/LiquidView", async () => ({
  GlassBlurTarget: (await import("react")).createContext<unknown>(undefined),
  LiquidView: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
  GlassPresence: ({ children }: { children?: ReactNode }) => children,
}));
vi.mock("@/ui/SceneFade", () => ({
  SceneFadeView: ({ children }: { children: unknown }) => children,
  useHeaderPush: () => ({}),
  useSceneKey: () => "/",
  hasNativeHeader: () => false,
  HEADER_PUSH_TOTAL: 350,
  GLASS_PUSH_DURATION: 0.36,
  PUSH_DONE_AFTER: 630,
}));
vi.mock("expo-router", () => ({
  usePathname: () => "/",
  useIsFocused: () => true,
  useGlobalSearchParams: () => ({}),
  router: { push: () => {} },
}));

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean })
  .IS_REACT_ACT_ENVIRONMENT = true;
vi.stubGlobal("__DEV__", false);

describe("persistent session frame", () => {
  it("samples Android page content without including the composer in its blur target", () => {
    layout.OS = "android";
    let target: unknown;
    let pageTarget: unknown;
    function Page() {
      pageTarget = use(GlassBlurTarget);
      return <span>Transcript</span>;
    }
    function Composer() {
      target = use(GlassBlurTarget);
      return <span data-composer>Composer</span>;
    }
    const container = document.createElement("div");
    const root = createRoot(container);
    try {
      act(() => root.render(
        <SessionFrame>
          <SessionFrame nativeHeaderHeight={80}>
            <SessionScreen composer={<Composer />}>
              <span data-header>Header declarations</span>
              <SessionContent><Page /></SessionContent>
            </SessionScreen>
          </SessionFrame>
        </SessionFrame>,
      ));
      const background = container.querySelector("[data-blur-target]")!;
      // The navigator's outer frame must not become a second recording target.
      expect(container.querySelectorAll("[data-blur-target]")).toHaveLength(1);
      expect(target).toBeUndefined();
      act(() => layout.onTargetLayout!());
      expect(background.textContent).toContain("Transcript");
      expect(background.querySelector("[data-composer]")).toBeNull();
      expect(background.querySelector("[data-header]")).toBeNull();
      expect((target as { current: unknown }).current).toBe(background);
      expect(pageTarget).toBeUndefined();
    } finally {
      act(() => root.unmount());
      layout.OS = "ios";
    }
  });
  it("reserves the full Android workspace line height below the native header", () => {
    layout.OS = "android";
    function Screen() {
      const { top, pageTop } = useSessionLayout();
      return (
        <SessionScreen below={<span>Workspace and changes</span>}>
          <output>{`${top}:${pageTop}`}</output>
        </SessionScreen>
      );
    }
    const container = document.createElement("div");
    const root = createRoot(container);
    try {
      act(() => root.render(<SessionFrame nativeHeaderHeight={80}><Screen /></SessionFrame>));
      expect(layout.onBelowLayout).toBeDefined();
      act(() => layout.onBelowLayout!({ nativeEvent: { layout: { height: 38 } } }));
      expect(container.textContent).toContain("Workspace and changes");
      expect(container.querySelector("output")?.textContent).toBe("156:128");
      // Larger font metrics must also move both content insets down.
      act(() => layout.onBelowLayout!({ nativeEvent: { layout: { height: 50 } } }));
      expect(container.querySelector("output")?.textContent).toBe("168:140");
    } finally {
      act(() => root.unmount());
      layout.OS = "ios";
      layout.onBelowLayout = undefined;
    }
  });

  it("opens a screen reading its own insets and updates its header without a render loop", () => {
    let renders = 0;
    function Screen({ title, compose }: { title: string; compose: boolean }) {
      renders += 1;
      const { top, bottom } = useSessionLayout();
      return (
        <SessionScreen
          title={title}
          action={{ icon: "list", label: "Sessions", onPress: () => {} }}
          composer={compose ? <span>Composer</span> : undefined}
        >
          <output>{`${top}:${bottom}`}</output>
        </SessionScreen>
      );
    }
    const container = document.createElement("div");
    const root = createRoot(container);
    const render = (title: string, compose: boolean) => act(() => root.render(
      <StrictMode>
        <SessionFrame><Screen title={title} compose={compose} /></SessionFrame>
      </StrictMode>,
    ));
    try {
      render("Settings", false);
      expect(container.textContent).toContain("Settings");
      expect(container.querySelector("output")?.textContent).toBe("114:12");
      render("Project", true);
      expect(container.textContent).toContain("Project");
      expect(container.textContent).toContain("Composer");
      expect(container.querySelector("output")?.textContent).toBe("114:28");
      render("Settings", false);
      expect(container.textContent).toContain("Settings");
      expect(container.textContent).not.toContain("Composer");
      expect(container.querySelector("output")?.textContent).toBe("114:12");
      expect(renders).toBeLessThan(20);
    } finally {
      act(() => root.unmount());
    }
  });
});
