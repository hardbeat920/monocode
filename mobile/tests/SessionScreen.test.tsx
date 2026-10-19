import { act, StrictMode, useState, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, it, vi } from "vitest";
import {
  SessionFrame,
  SessionScreen,
  useSessionLayout,
} from "@/features/sessions/SessionScreen";
vi.mock("expo-sqlite/kv-store", () => ({ default: { getItemSync: () => null } }));

vi.mock("react-native", () => {
  const View = ({ children }: { children?: ReactNode }) => <div>{children}</div>;
  return {
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
vi.mock("@/ui/LiquidView", () => ({
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
