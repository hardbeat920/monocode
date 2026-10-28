// @vitest-environment happy-dom
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { prepareQuickComposerWhenIdle } from "./prepareQuickComposer";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
let focused: boolean;
let hidden: boolean;
let nextId: number;
let frames: Map<number, FrameRequestCallback>;
let idle: Map<number, () => void>;
let stop: (() => void) | undefined;
function paint() {
  const callbacks = [...frames.values()];
  frames.clear();
  callbacks.forEach((callback) => callback(0));
}
async function runIdle() {
  const callbacks = [...idle.values()];
  idle.clear();
  callbacks.forEach((callback) => callback());
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
}
beforeEach(() => {
  focused = true;
  hidden = false;
  nextId = 0;
  frames = new Map();
  idle = new Map();
  vi.mocked(invoke).mockResolvedValue(true);
  vi.spyOn(document, "hasFocus").mockImplementation(() => focused);
  vi.spyOn(document, "hidden", "get").mockImplementation(() => hidden);
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
    frames.set(++nextId, callback);
    return nextId;
  });
  vi.stubGlobal("cancelAnimationFrame", (id: number) => frames.delete(id));
  vi.stubGlobal("requestIdleCallback", (callback: () => void) => {
    idle.set(++nextId, callback);
    return nextId;
  });
  vi.stubGlobal("cancelIdleCallback", (id: number) => idle.delete(id));
});
afterEach(() => {
  stop?.();
  stop = undefined;
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  vi.clearAllMocks();
  vi.useRealTimers();
});

it("waits for a paint and idle time, then prepares only once", async () => {
  stop = prepareQuickComposerWhenIdle();
  expect(invoke).not.toHaveBeenCalled();
  paint();
  expect(idle.size).toBe(0);
  paint();
  expect(invoke).not.toHaveBeenCalled();
  await runIdle();
  expect(invoke).toHaveBeenCalledExactlyOnceWith("quick_composer_prepare");
  window.dispatchEvent(new Event("focus"));
  paint();
  paint();
  await runIdle();
  expect(invoke).toHaveBeenCalledTimes(1);
});

it("cancels pending work when focus leaves and resumes after focus returns", async () => {
  stop = prepareQuickComposerWhenIdle();
  paint();
  paint();
  focused = false;
  window.dispatchEvent(new Event("blur"));
  await runIdle();
  expect(invoke).not.toHaveBeenCalled();
  focused = true;
  window.dispatchEvent(new Event("focus"));
  paint();
  paint();
  await runIdle();
  expect(invoke).toHaveBeenCalledOnce();
});

it("waits for a hidden workspace and retries when the native focus check defers preparation", async () => {
  hidden = true;
  stop = prepareQuickComposerWhenIdle();
  expect(frames.size).toBe(0);
  hidden = false;
  document.dispatchEvent(new Event("visibilitychange"));
  vi.mocked(invoke).mockResolvedValueOnce(false);
  paint();
  paint();
  await runIdle();
  window.dispatchEvent(new Event("focus"));
  paint();
  paint();
  await runIdle();
  expect(invoke).toHaveBeenCalledTimes(2);
});

it("cancels work on cleanup, including a StrictMode remount", async () => {
  const first = prepareQuickComposerWhenIdle();
  paint();
  first();
  stop = prepareQuickComposerWhenIdle();
  paint();
  paint();
  await runIdle();
  expect(invoke).toHaveBeenCalledOnce();
  stop();
  window.dispatchEvent(new Event("focus"));
  expect(frames.size).toBe(0);
});

it("supports WebKit without requestIdleCallback without running before paint", async () => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
  vi.stubGlobal("requestIdleCallback", undefined);
  stop = prepareQuickComposerWhenIdle();
  await vi.runAllTimersAsync();
  expect(invoke).not.toHaveBeenCalled();
  paint();
  paint();
  expect(invoke).not.toHaveBeenCalled();
  await vi.runAllTimersAsync();
  expect(invoke).toHaveBeenCalledOnce();
});
