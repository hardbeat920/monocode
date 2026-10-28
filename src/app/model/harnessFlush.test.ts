// @vitest-environment happy-dom
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { cancelScheduledFlush, scheduleHarnessFlush } from "./harnessFlush";

beforeEach(() => {
  vi.useFakeTimers();
  vi.spyOn(document, "hidden", "get").mockReturnValue(false);
});
afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

it("batches background-only work while allowing it to progress", () => {
  const flush = vi.fn();
  const handle = scheduleHarnessFlush(flush, false);
  expect(handle.kind).toBe("timeout");
  vi.advanceTimersByTime(99);
  expect(flush).not.toHaveBeenCalled();
  vi.advanceTimersByTime(1);
  expect(flush).toHaveBeenCalledTimes(1);
});

it("promotes newly visible output to the next frame without a duplicate flush", () => {
  const frame = vi.spyOn(window, "requestAnimationFrame");
  const flush = vi.fn();
  const background = scheduleHarnessFlush(flush, false);
  cancelScheduledFlush(background);
  const foreground = scheduleHarnessFlush(flush, true);
  expect(foreground.kind).toBe("raf");
  expect(frame).toHaveBeenCalledTimes(1);
  cancelScheduledFlush(foreground);
  vi.advanceTimersByTime(200);
  expect(flush).not.toHaveBeenCalled();
});

it("does not depend on animation frames while the window is hidden", () => {
  vi.spyOn(document, "hidden", "get").mockReturnValue(true);
  const flush = vi.fn();
  expect(scheduleHarnessFlush(flush, true).kind).toBe("timeout");
  vi.advanceTimersByTime(100);
  expect(flush).toHaveBeenCalledTimes(1);
});
