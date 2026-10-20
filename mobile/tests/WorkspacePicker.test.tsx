import { act, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { WorkspacePicker } from "@/features/connection/WorkspacePicker";

const app = vi.hoisted(() => ({ os: "ios" }));
vi.mock("expo-haptics", () => ({ selectionAsync: vi.fn() }));
vi.mock("react-native", () => ({
  Platform: { get OS() { return app.os; } },
  StyleSheet: { create: (styles: object) => styles, hairlineWidth: 1 },
  View: ({ children, style }: { children: ReactNode; style?: object }) => <div data-view style={style}>{children}</div>,
  Text: ({ children }: { children: ReactNode }) => <span>{children}</span>,
  ScrollView: ({ children, contentContainerStyle, contentInsetAdjustmentBehavior }: {
    children: ReactNode; contentContainerStyle: object; contentInsetAdjustmentBehavior: string;
  }) => <main style={contentContainerStyle} data-insets={contentInsetAdjustmentBehavior}>{children}</main>,
  Pressable: ({ children, onPress }: { children: ReactNode; onPress: () => void }) => <button onClick={onPress}>{children}</button>,
  ActivityIndicator: () => null,
}));
vi.mock("react-native-safe-area-context", () => ({ useSafeAreaInsets: () => ({ top: 24, bottom: 16 }) }));
vi.mock("@/features/connection/ConnectionProvider", () => ({
  useConnection: () => ({ connected: true, selection: { projectIds: [], monoIds: [] },
    catalog: { projects: [], monos: [] }, select: vi.fn(), refresh: vi.fn() }),
}));
vi.mock("@/features/connection/ConnectScreen", () => ({ connectionStyleSheet: () => ({}) }));
vi.mock("@/features/mascots/PixelMascot", () => ({ MonoMascot: () => null }));
vi.mock("@/features/mascots/ProjectMascot", () => ({ ProjectMascot: () => null }));
vi.mock("@/features/settings/SettingsUI", () => ({ SettingsRow: () => null }));
vi.mock("@/ui/GradualBlur", () => ({ GradualBlur: () => null }));
vi.mock("@/ui/Icon", () => ({ Icon: () => null }));
vi.mock("@/ui/LiquidView", () => ({ LiquidView: ({ children }: { children: ReactNode }) => <div>{children}</div> }));
vi.mock("@/ui/theme", () => ({
  themedStyles: (create: (colors: object) => object) => () => create({}),
  useColors: () => ({ sheet: "black" }), withAlpha: () => "black", radii: { card: 20 },
}));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
let root: ReturnType<typeof createRoot>;
afterEach(() => { act(() => root.unmount()); app.os = "ios"; });
function render(editing: boolean) {
  const container = document.createElement("div");
  root = createRoot(container);
  act(() => root.render(<WorkspacePicker editing={editing} headerHeight={80} />));
  return container;
}
it("puts the iOS editing scroll view at the screen root, like Settings", () => {
  const container = render(true);
  expect(container.firstElementChild?.tagName).toBe("MAIN");
  expect(container.querySelector("main")?.dataset.insets).toBe("automatic");
  expect(container.querySelector("main")?.style.paddingTop).toBe("4px");
});
it("reserves Android's transparent header height before project rows", () => {
  app.os = "android";
  const container = render(true);
  expect(container.querySelector("main")?.dataset.insets).toBe("never");
  expect(container.querySelector("main")?.style.paddingTop).toBe("92px");
});
it("retains the standalone workspace screen wrapper and safe-area spacing", () => {
  const container = render(false);
  expect(container.firstElementChild?.tagName).toBe("DIV");
  expect(container.querySelector("main")?.style.paddingTop).toBe("72px");
});
