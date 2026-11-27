import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

const native = vi.hoisted(() => ({
  os: "ios",
  version: undefined as number | undefined,
  requireView: vi.fn(),
}));
vi.mock("expo", () => ({
  requireOptionalNativeModule: () => ({ typingPostureVersion: native.version }),
  requireNativeView: native.requireView,
}));
vi.mock("react-native", () => ({
  Platform: { get OS() { return native.os; } },
  StyleSheet: { absoluteFill: {} },
}));

const container = document.createElement("div");
let root = createRoot(container);
beforeEach(() => {
  (globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  vi.resetModules();
  native.requireView.mockReset().mockReturnValue(() => <div data-native-posture />);
  native.os = "ios";
  native.version = undefined;
});
afterEach(() => {
  act(() => root.unmount());
  root = createRoot(container);
});

it.each(["ios", "android"])("omits hinge views on %s without native support", async (os) => {
  native.os = os;
  // Even if a future Android module advertises it, iOS's view isn't loaded.
  native.version = os === "android" ? 1 : undefined;
  const { TypingPostureFocus } = await import("@/features/sessions/TypingPostureFocus");
  act(() => root.render(<TypingPostureFocus enabled onEnterTypingPosture={() => {}} />));
  expect(container.childElementCount).toBe(0);
  expect(native.requireView).not.toHaveBeenCalled();
});

it("loads the named posture view only in a supporting iOS build", async () => {
  native.version = 1;
  const { TypingPostureFocus } = await import("@/features/sessions/TypingPostureFocus");
  act(() => root.render(<TypingPostureFocus enabled onEnterTypingPosture={() => {}} />));
  expect(container.querySelector("[data-native-posture]")).not.toBeNull();
  expect(native.requireView).toHaveBeenCalledWith("MonocodeLink", "TypingPostureView");
});
