import { act, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { UsageLimitCard } from "@/features/sessions/UsageLimitCard";
import type { UsageLimitState } from "@/features/workspace/types";

const desktop = vi.hoisted(() => ({ connected: true, recoverUsage: vi.fn() }));
vi.mock("@/features/connection/ConnectionProvider", () => ({
  useConnection: () => desktop,
}));
vi.mock("@/features/workspace/WorkspaceProvider", () => ({
  useWorkspace: () => desktop,
}));
vi.mock("@/ui/theme", () => ({ themedStyles: () => () => ({}) }));
vi.mock("react-native", () => ({
  View: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
  Text: ({ children }: { children?: ReactNode }) => <span>{children}</span>,
  Pressable: ({
    children,
    onPress,
    disabled,
  }: {
    children?: ReactNode;
    onPress?: () => void;
    disabled?: boolean;
  }) => (
    <button onClick={onPress} disabled={disabled}>
      {children}
    </button>
  ),
}));
(
  globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean }
).IS_REACT_ACT_ENVIRONMENT = true;
let container: HTMLDivElement;
let root: ReturnType<typeof createRoot>;
const state: UsageLimitState = {
  token: "paused",
  resetsAt: 123456,
  canResume: true,
  canAutoResume: true,
  resumeAtReset: false,
  resetLabel: "Resets tomorrow at 9 AM",
  limitation: "Keep Monocode running on desktop",
};
const render = (patch: Partial<UsageLimitState> = {}) =>
  act(() =>
    root.render(
      <UsageLimitCard
        conversationId="mono:fixture"
        state={{ ...state, ...patch }}
      />,
    ),
  );
const buttons = () => Array.from(container.querySelectorAll("button"));
beforeEach(() => {
  vi.clearAllMocks();
  desktop.connected = true;
  desktop.recoverUsage.mockResolvedValue(undefined);
  container = document.createElement("div");
  root = createRoot(container);
});
afterEach(() => act(() => root.unmount()));
it("renders desktop reset data and invokes desktop manual and automatic actions for Mono conversations", async () => {
  render();
  expect(container.textContent).toContain(state.resetLabel);
  expect(container.textContent).toContain(state.limitation);
  await act(async () => {
    buttons()[0].click();
  });
  expect(desktop.recoverUsage).toHaveBeenLastCalledWith(
    "mono:fixture",
    state,
    undefined,
  );
  await act(async () => {
    buttons()[1].click();
  });
  expect(desktop.recoverUsage).toHaveBeenLastCalledWith(
    "mono:fixture",
    state,
    true,
  );
  render({ resumeAtReset: true });
  await act(async () => {
    buttons()[1].click();
  });
  expect(desktop.recoverUsage).toHaveBeenLastCalledWith(
    "mono:fixture",
    { ...state, resumeAtReset: true },
    false,
  );
});
it("uses desktop eligibility for early attempts, unknown resets and unsupported sessions", () => {
  render({ canResume: false });
  expect(buttons()[0].disabled).toBe(true);
  render({
    canAutoResume: false,
    resetsAt: null,
    resetLabel: "Reset time unknown",
  });
  expect(buttons()[0].disabled).toBe(false);
  expect(buttons()[1].disabled).toBe(true);
  expect(container.textContent).toContain("Reset time unknown");
  render({
    canResume: false,
    canAutoResume: false,
    limitation: "Continue on remote desktop",
  });
  expect(buttons().every((button) => button.disabled)).toBe(true);
  desktop.connected = false;
  render();
  expect(buttons().every((button) => button.disabled)).toBe(true);
  expect(container.textContent).toContain("Reconnect");
});
it("suppresses duplicate taps, reports errors and permits retry", async () => {
  let reject!: (reason: Error) => void;
  desktop.recoverUsage.mockReturnValueOnce(
    new Promise((_, fail) => {
      reject = fail;
    }),
  );
  render();
  act(() => {
    buttons()[0].click();
    buttons()[0].click();
  });
  expect(desktop.recoverUsage).toHaveBeenCalledTimes(1);
  expect(buttons().every((button) => button.disabled)).toBe(true);
  await act(async () => {
    reject(new Error("Desktop is still opening. Try again."));
  });
  expect(container.textContent).toContain("Desktop is still opening");
  await act(async () => {
    buttons()[0].click();
  });
  expect(desktop.recoverUsage).toHaveBeenCalledTimes(2);
  expect(container.textContent).not.toContain("Desktop is still opening");
});
