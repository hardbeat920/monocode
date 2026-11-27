import { act, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { androidBackHeader } from "@/ui/AndroidHeader";

const app = vi.hoisted(() => ({ os: "android", back: vi.fn() }));
vi.mock("expo-router", () => ({ router: { back: app.back } }));
vi.mock("react-native", () => ({
  Platform: { get OS() { return app.os; } },
  View: ({ children }: { children: ReactNode }) => <div>{children}</div>,
  Text: ({ children }: { children: ReactNode }) => <span>{children}</span>,
  Pressable: ({ children, onPress, accessibilityLabel }: {
    children: ReactNode; onPress: () => void; accessibilityLabel: string;
  }) => <button aria-label={accessibilityLabel} onClick={onPress}>{children}</button>,
}));
vi.mock("react-native-svg", () => ({ default: () => <svg />, Path: () => null }));
vi.mock("@/ui/theme", () => ({
  useColors: () => ({ content: "white" }),
  themedStyles: (create: (colors: object) => object) => () => create({}),
}));
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

it("uses one native title slot for both back and text, leaving no empty sibling slot", () => {
  const options = androidBackHeader("Appearance");
  expect(options.headerBackVisible).toBe(false);
  expect(options.headerLeft).toBeUndefined();
  const title = options.headerTitle as () => ReactNode;
  const container = document.createElement("div");
  const root = createRoot(container);
  try {
    act(() => root.render(title()));
    expect(container.textContent).toBe("Appearance");
    expect(container.querySelectorAll("button")).toHaveLength(1);
    act(() => container.querySelector("button")!.click());
    expect(app.back).toHaveBeenCalledOnce();
  } finally { act(() => root.unmount()); }
});
it("preserves iOS native header options", () => {
  app.os = "ios";
  try { expect(androidBackHeader("Appearance")).toEqual({}); }
  finally { app.os = "android"; }
});
