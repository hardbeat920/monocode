import { act, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { useSafeAreaInsets } from "react-native-safe-area-context";
import { OcclusionSafeArea } from "@/ui/OcclusionSafeArea";
import type { OcclusionMeasurement } from "@/ui/occlusionInsets";

const native = vi.hoisted(() => ({
  emit: undefined as undefined | ((event: { nativeEvent: OcclusionMeasurement }) => void),
  emitters: [] as ((event: { nativeEvent: OcclusionMeasurement }) => void)[],
}));
vi.mock("@/features/connection/nativeLink", () => ({ hasOcclusionObserver: true }));
vi.mock("expo", () => ({
  requireNativeView: () => ({ onOcclusionChange }: { onOcclusionChange: typeof native.emit }) => {
    native.emit = onOcclusionChange;
    if (onOcclusionChange) native.emitters.push(onOcclusionChange);
    return null;
  },
}));
vi.mock("react-native", () => ({
  View: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
  StyleSheet: { absoluteFill: {}, create: (styles: object) => styles },
}));
vi.mock("react-native-safe-area-context", async () => {
  const { createContext, use } = await import("react");
  const context = createContext({ top: 0, bottom: 12, left: 0, right: 84 });
  return { SafeAreaInsetsContext: context, useSafeAreaInsets: () => use(context) };
});

const container = document.createElement("div");
const root = createRoot(container);
(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
beforeEach(() => { native.emitters = []; });
afterEach(() => act(() => root.render(null)));
function Control() {
  const { left, right, bottom } = useSafeAreaInsets();
  return <button>{`${left}:${right}:${bottom}`}</button>;
}
const report = (supported: boolean, regions: OcclusionMeasurement["regions"] = []) =>
  act(() => native.emit!({ nativeEvent: { supported, width: 469, height: 160, regions } }));

it("keeps safe clearance until measurements arrive and on unsupported OS versions", () => {
  act(() => root.render(<OcclusionSafeArea><Control /></OcclusionSafeArea>));
  expect(container.textContent).toBe("0:84:12");
  report(false);
  expect(container.textContent).toBe("0:84:12");
});

it("expands when clear, aligns to the toolbar when obstructed, and preserves vertical insets", () => {
  act(() => root.render(<OcclusionSafeArea><Control /></OcclusionSafeArea>));
  report(true);
  expect(container.textContent).toBe("0:0:12");
  report(true, [{ x: 400, y: 50, width: 69, height: 100 }]);
  expect(container.textContent).toBe("0:84:12");
  report(true, [{ x: 365, y: 0, width: 104, height: 160 }]);
  expect(container.textContent).toBe("0:104:12");
  report(true);
  expect(container.textContent).toBe("0:0:12");
});

it("accepts conservative fallback and later fresh geometry without remounting", () => {
  act(() => root.render(<OcclusionSafeArea><Control /></OcclusionSafeArea>));
  report(true);
  expect(container.textContent).toBe("0:0:12");
  report(false);
  expect(container.textContent).toBe("0:84:12");
  report(true, [{ x: 385, y: -300, width: 84, height: 100 }]);
  expect(container.textContent).toBe("0:0:12");
});


it("keeps clear finished and expanded groups wide when only the now-playing group is obstructed", () => {
  act(() => root.render(<>
    <OcclusionSafeArea><Control /></OcclusionSafeArea>
    <OcclusionSafeArea><Control /></OcclusionSafeArea>
    <OcclusionSafeArea><Control /></OcclusionSafeArea>
  </>));
  const [finished, expanded, playing] = native.emitters;
  act(() => {
    finished({ nativeEvent: { supported: true, width: 469, height: 60, regions: [] } });
    expanded({ nativeEvent: { supported: true, width: 469, height: 120, regions: [] } });
    playing({ nativeEvent: { supported: true, width: 469, height: 60,
      regions: [{ x: 385, y: 0, width: 84, height: 60 }] } });
  });
  expect([...container.querySelectorAll("button")].map((button) => button.textContent)).toEqual([
    "0:0:12", "0:0:12", "0:84:12",
  ]);
});
