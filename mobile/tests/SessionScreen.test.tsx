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
  useKeyboardLift,
} from "@/features/sessions/SessionScreen";
const layout = vi.hoisted(() => ({
  OS: "ios",
  windowHeight: 1000,
  frameHandler: undefined as undefined | ((event: { endCoordinates: { screenY: number; height: number } }) => void),
  keyboardHandler: undefined as undefined | {
    onStart: () => void;
    onEnd: (event: { height: number; progress: number }) => void;
  },
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
    Dimensions: { get: () => ({ height: layout.windowHeight }) },
    Keyboard: { addListener: (_name: string, handler: typeof layout.frameHandler) => {
      layout.frameHandler = handler;
      return { remove: () => { layout.frameHandler = undefined; } };
    } },
    View,
    Text: View,
    StyleSheet: { create: (styles: unknown) => styles },
    useColorScheme: () => "dark",
  };
});
vi.mock("react-native-keyboard-controller", () => ({
  KeyboardGestureArea: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
  useKeyboardHandler: (handler: typeof layout.keyboardHandler) => { layout.keyboardHandler = handler; },
}));
vi.mock("react-native-reanimated", () => ({
  default: { View: ({ children }: { children?: ReactNode }) => <div>{children}</div> },
  Easing: { out: () => {}, cubic: () => {} },
  interpolate: () => 0,
  useAnimatedStyle: (read: () => object) => read(),
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
// The drawer's pager and its loading cover reach native modules; here the
// drawer is shut and no screen switch is under way.
vi.mock("@/ui/ScenePager", () => ({
  useDrawerActions: () => ({
    open: vi.fn(),
    close: vi.fn(),
    closeThen: (change: () => void) => change(),
    allowSwipe: () => () => {},
  }),
  useDrawerSwipe: vi.fn(),
  useSceneSwapping: () => false,
  useSceneSwitching: () => false,
}));
vi.mock("@/ui/DitherLoading", () => ({ DitherLoading: () => null }));
vi.mock("@/features/workspace/UnreadDot", () => ({
  UnreadDot: () => null,
  useAnyUnreadMono: () => false,
}));
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
      expect(container.querySelector("output")?.textContent).toBe("108:12");
      render("Project", true);
      expect(container.textContent).toContain("Project");
      expect(container.textContent).toContain("Composer");
      expect(container.querySelector("output")?.textContent).toBe("108:28");
      render("Settings", false);
      expect(container.textContent).toContain("Settings");
      expect(container.textContent).not.toContain("Composer");
      expect(container.querySelector("output")?.textContent).toBe("108:12");
      expect(renders).toBeLessThan(20);
    } finally {
      act(() => root.unmount());
    }
  });
});

it("keeps the composer above the settled iOS keyboard through rotation and stale animation completions", () => {
  layout.OS = "ios";
  layout.windowHeight = 1000;
  const container = document.createElement("div");
  const root = createRoot(container);
  function Probe() {
    const style = useKeyboardLift(0) as unknown as { transform: { translateY: number }[] };
    return <span data-lift={style.transform[0].translateY} />;
  }
  const render = () => act(() => root.render(<Probe />));
  const lift = () => container.querySelector("[data-lift]")!.getAttribute("data-lift");
  try {
    render();
    act(() => layout.frameHandler!({ endCoordinates: { screenY: 600, height: 400 } }));
    render();
    expect(lift()).toBe("-400");
    layout.windowHeight = 600;
    act(() => layout.frameHandler!({ endCoordinates: { screenY: 400, height: 200 } }));
    act(() => layout.keyboardHandler!.onEnd({ height: 0, progress: 0 }));
    render();
    expect(lift()).toBe("-200");
    // A real dismissal still wins, even if an old show animation ends later.
    act(() => layout.frameHandler!({ endCoordinates: { screenY: 600, height: 200 } }));
    act(() => layout.keyboardHandler!.onEnd({ height: 400, progress: 1 }));
    render();
    expect(lift()).toBe("0");
  } finally {
    act(() => root.unmount());
  }
});

it("does not apply a previous transition's settled iOS keyboard when onEnd beats the frame event", () => {
  layout.OS = "ios";
  layout.windowHeight = 1000;
  const container = document.createElement("div");
  const root = createRoot(container);
  function Probe() {
    const style = useKeyboardLift(0) as unknown as { transform: { translateY: number }[] };
    return <span data-lift={style.transform[0].translateY} />;
  }
  const render = () => act(() => root.render(<Probe />));
  const lift = () => container.querySelector("[data-lift]")!.getAttribute("data-lift");
  try {
    render();
    act(() => layout.frameHandler!({ endCoordinates: { screenY: 600, height: 400 } }));
    // Dismissal: onEnd lands before UIKit's settled frame.
    act(() => layout.keyboardHandler!.onStart());
    act(() => layout.keyboardHandler!.onEnd({ height: 0, progress: 0 }));
    render();
    expect(lift()).toBe("0");
    act(() => layout.frameHandler!({ endCoordinates: { screenY: 1000, height: 400 } }));
    render();
    expect(lift()).toBe("0");
    // Show again: the dismissed overlap must not hold the composer down.
    act(() => layout.keyboardHandler!.onStart());
    act(() => layout.keyboardHandler!.onEnd({ height: 300, progress: 1 }));
    render();
    expect(lift()).toBe("-300");
    act(() => layout.frameHandler!({ endCoordinates: { screenY: 700, height: 300 } }));
    render();
    expect(lift()).toBe("-300");
  } finally {
    act(() => root.unmount());
  }
});
