// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { newSession } from "../../features/sessions/model/session";
import { applyHarnessEvents } from "../../integrations/harness/core/apply";
import type { HarnessEvent } from "../../integrations/harness/core/types";
import {
  cancelScheduledFlush,
  HarnessEventQueue,
  scheduleHarnessFlush,
} from "./harnessFlush";

beforeEach(() => {
  vi.useFakeTimers();
  vi.spyOn(document, "hidden", "get").mockReturnValue(false);
});
afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
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

function controlledQueue(foreground: Set<string>) {
  const frames = new Map<number, FrameRequestCallback>();
  let nextFrame = 0;
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
    frames.set(++nextFrame, callback);
    return nextFrame;
  });
  vi.stubGlobal("cancelAnimationFrame", (id: number) => frames.delete(id));
  const apply = vi.fn<(batches: ReadonlyMap<string, HarnessEvent[]>) => void>();
  const queue = new HarnessEventQueue((id) => foreground.has(id), apply);
  const paint = () => {
    const pending = [...frames.values()];
    frames.clear();
    for (const callback of pending) callback(performance.now());
  };
  return { queue, apply, paint, frames };
}

describe("harness event queue", () => {
  it("keeps two background streams throttled during continuous foreground output", () => {
    const { queue, apply, paint } = controlledQueue(new Set(["front"]));
    for (let i = 0; i < 20; i++) {
      queue.enqueue("back1", { type: "message.delta", text: ` b${i}` });
      queue.enqueue("back2", { type: "message.delta", text: ` c${i}` });
      queue.enqueue("front", { type: "message.delta", text: ` a${i}` });
      paint();
      vi.advanceTimersByTime(16);
    }
    expect(
      apply.mock.calls.filter(([batch]) => batch.has("front")),
    ).toHaveLength(20);
    expect(
      apply.mock.calls.filter(([batch]) => batch.has("back1")),
    ).toHaveLength(2);
    for (const [batch] of apply.mock.calls) {
      if (batch.has("front")) expect([...batch.keys()]).toEqual(["front"]);
      else expect([...batch.keys()]).toEqual(["back1", "back2"]);
    }
    vi.advanceTimersByTime(100);
    for (const id of ["front", "back1", "back2"]) {
      const events = apply.mock.calls.flatMap(([batch]) => batch.get(id) ?? []);
      expect(events).toHaveLength(20);
      expect(
        events.map((event) => event.type === "message.delta" && event.text),
      ).toEqual(
        Array.from(
          { length: 20 },
          (_, i) => ` ${{ front: "a", back1: "b", back2: "c" }[id]}${i}`,
        ),
      );
    }
    expect(vi.getTimerCount()).toBe(0);
  });

  it("catches up a selected chat without flushing other chats or postponing their timer", () => {
    const foreground = new Set(["front"]);
    const { queue, apply } = controlledQueue(foreground);
    queue.enqueue("back1", { type: "message.delta", text: "Selected" });
    queue.enqueue("back2", { type: "message.delta", text: "Hidden" });
    vi.advanceTimersByTime(40);
    foreground.clear();
    foreground.add("back1");
    queue.flushForeground();
    expect(apply).toHaveBeenCalledOnce();
    expect([...apply.mock.calls[0][0].keys()]).toEqual(["back1"]);
    vi.advanceTimersByTime(59);
    expect(apply).toHaveBeenCalledOnce();
    vi.advanceTimersByTime(1);
    expect([...apply.mock.calls[1][0].keys()]).toEqual(["back2"]);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("moves output from the previous foreground chat onto the background schedule", () => {
    const foreground = new Set(["front"]);
    const { queue, apply, frames, paint } = controlledQueue(foreground);
    queue.enqueue("front", { type: "message.delta", text: "Still queued" });
    foreground.clear();
    foreground.add("other");
    queue.flushForeground();
    expect(frames.size).toBe(0);
    paint();
    expect(apply).not.toHaveBeenCalled();
    vi.advanceTimersByTime(100);
    expect(apply.mock.calls[0][0].get("front")).toEqual([
      { type: "message.delta", text: "Still queued" },
    ]);
  });

  it.each<HarnessEvent>([
    { type: "approval.requested", callId: "tool", requestId: 1, title: "Run" },
    { type: "approval.resolved", requestId: 1, decision: "allow" },
    { type: "question.asked", requestId: 2, questions: [] },
    { type: "question.resolved", requestId: 2 },
  ])(
    "delivers $type immediately after that session's preceding output",
    (event) => {
      const { queue, apply } = controlledQueue(new Set());
      const delta: HarnessEvent = {
        type: "message.delta",
        text: "Before prompt",
      };
      queue.enqueue("prompt", delta);
      queue.enqueue("other", { type: "message.delta", text: "Unrelated" });
      queue.enqueue("prompt", event);
      expect(apply).toHaveBeenCalledOnce();
      expect([...apply.mock.calls[0][0]]).toEqual([["prompt", [delta, event]]]);
      vi.advanceTimersByTime(100);
      expect([...apply.mock.calls[1][0].keys()]).toEqual(["other"]);
    },
  );

  it("advances hidden-window output even when animation frames are suspended", () => {
    const { queue, apply, frames } = controlledQueue(new Set(["front"]));
    queue.enqueue("front", { type: "message.delta", text: "Visible first" });
    vi.spyOn(document, "hidden", "get").mockReturnValue(true);
    queue.flush(); // The visibilitychange handler flushes before WebKit suspends.
    queue.enqueue("front", { type: "message.delta", text: "Now hidden" });
    expect(frames.size).toBe(0);
    vi.advanceTimersByTime(100);
    expect(apply.mock.calls[1][0].get("front")).toEqual([
      { type: "message.delta", text: "Now hidden" },
    ]);
  });

  it("flushes all output at lifecycle boundaries without delivering it twice", () => {
    const { queue, apply, frames } = controlledQueue(new Set(["front"]));
    queue.enqueue("front", { type: "message.delta", text: "Visible" });
    queue.enqueue("back", { type: "message.delta", text: "Hidden" });
    queue.flush();
    expect([...apply.mock.calls[0][0].keys()]).toEqual(["front", "back"]);
    expect(frames.size).toBe(0);
    vi.advanceTimersByTime(1000);
    queue.flush();
    expect(apply).toHaveBeenCalledOnce();
  });

  it("cancels both schedules when the window runtime is cleaned up", () => {
    const { queue, apply, frames } = controlledQueue(new Set(["front"]));
    queue.enqueue("front", { type: "message.delta", text: "Visible" });
    queue.enqueue("back", { type: "message.delta", text: "Hidden" });
    queue.cancelScheduled();
    expect(frames.size).toBe(0);
    vi.advanceTimersByTime(1000);
    expect(apply).not.toHaveBeenCalled();
    queue.flush();
    expect([...apply.mock.calls[0][0].keys()]).toEqual(["front", "back"]);
  });
});

it("applies a wake queued during a checkpoint before finalization resumes", async () => {
  let session = newSession("pi", "/repo");
  const queue = new HarnessEventQueue(
    () => false,
    (batches) => {
      session = applyHarnessEvents(session, batches.get(session.id) ?? []);
    },
  );
  let finish!: () => void;
  const checkpoint = new Promise<void>((resolve) => {
    finish = resolve;
  });
  const flushed = queue.flushAfter(checkpoint);
  queue.enqueue(session.id, { type: "turn.activity", active: true });
  queue.enqueue(session.id, { type: "message.delta", text: "New run" });
  expect(session.providerActive).toBeUndefined();
  finish();
  await flushed;
  expect(session.providerActive).toBe(true);
  expect(session.busy).toBe(true);
  expect(session.blocks.at(-1)?.text).toBe("New run");
  expect(vi.getTimerCount()).toBe(0);
});
