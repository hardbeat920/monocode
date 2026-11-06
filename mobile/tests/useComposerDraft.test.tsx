import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const world = vi.hoisted(() => ({
  focused: true,
  appState: "active",
  listeners: new Set<(state: string) => void>(),
  command: vi.fn(),
}));
vi.mock("react-native", () => ({
  AppState: {
    get currentState() {
      return world.appState;
    },
    addEventListener: (_: string, listener: (state: string) => void) => {
      world.listeners.add(listener);
      return { remove: () => world.listeners.delete(listener) };
    },
  },
}));
vi.mock("expo-router", () => ({ useIsFocused: () => world.focused }));
vi.mock("@/features/connection/ConnectionProvider", () => ({
  useConnection: () => ({ command: world.command }),
}));
vi.mock("@/features/connection/nativeLink", () => ({ makeCommandId: () => "id" }));

import { useComposerDraft } from "@/features/sessions/useComposerDraft";

(
  globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean }
).IS_REACT_ACT_ENVIRONMENT = true;

const show = () => {};
function Composer({ id }: { id: string }) {
  useComposerDraft(id, show);
  return null;
}
let root: ReturnType<typeof createRoot>;
const run = (ms: number) => act(async () => void (await vi.advanceTimersByTimeAsync(ms)));
const reads = () => world.command.mock.calls.filter(([method]) => method === "composer.read").length;
const mount = (id: string) => act(async () => root.render(<Composer id={id} />));
const never = () => new Promise(() => {});

beforeEach(() => {
  vi.useFakeTimers();
  world.focused = true;
  world.appState = "active";
  world.listeners.clear();
  world.command.mockReset();
  root = createRoot(document.createElement("div"));
});
afterEach(() => {
  act(() => root.unmount());
  vi.useRealTimers();
});

describe("useComposerDraft long poll", () => {
  it("does not wake in the background, and reads again once shown", async () => {
    world.appState = "background";
    world.command.mockImplementation(never);
    await mount("bg");
    await run(120_000);
    expect(reads()).toBe(0);
    expect(vi.getTimerCount()).toBe(0);
    world.appState = "active";
    await act(async () => {
      for (const listener of [...world.listeners]) listener("active");
    });
    await run(0);
    expect(reads()).toBe(1);
  });

  it("does not follow a thread whose screen is not focused", async () => {
    world.focused = false;
    world.command.mockImplementation(never);
    await mount("blurred");
    await run(60_000);
    expect(reads()).toBe(0);
  });

  it("waits longer after each failed read", async () => {
    world.command.mockRejectedValue(new Error("offline"));
    await mount("offline");
    await run(300_000);
    // A fixed 2 s retry would be 150 reads; doubling to 30 s is about 14.
    expect(reads()).toBeGreaterThan(5);
    expect(reads()).toBeLessThan(25);
  });
});
