// @vitest-environment happy-dom
import { act, createElement, useLayoutEffect } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import type { Block } from "./session";

afterEach(() => {
  localStorage.removeItem("monocode.measureStreaming");
  vi.unstubAllGlobals();
  vi.resetModules();
});

it("reports receive-to-commit separately from delayed reveal and excludes hidden output", async () => {
  localStorage.setItem("monocode.measureStreaming", "1");
  let now = 0;
  const measure = vi.fn();
  const frames: FrameRequestCallback[] = [];
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal("performance", {
    now: () => now,
    measure,
    getEntriesByName: () => [],
    clearMeasures: () => {},
  });
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
    frames.push(callback);
    return frames.length;
  });
  const {
    noteStreamArrival,
    noteStreamApplied,
    streamMeasurement,
    useStreamPresentation,
  } = await import("./streamingPerformance");
  const block: Block = {
    id: "reply",
    role: "assistant",
    text: "hello",
    streaming: true,
  };
  const event = { type: "message.delta", text: "hello" } as const;
  noteStreamArrival(event);
  now = 10;
  noteStreamApplied([], [block], [event], true);
  const observation = streamMeasurement(block)!;
  function Probe({
    shown,
    visible = true,
  }: {
    shown: number;
    visible?: boolean;
  }) {
    const committed = useStreamPresentation(observation, 5, visible);
    useLayoutEffect(() => committed(shown), [committed, shown]);
    return createElement("span", null, block.text.slice(0, shown));
  }
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(createElement(Probe, { shown: 0 })));
    now = 210;
    expect(measure).not.toHaveBeenCalled();
    await act(async () => root.render(createElement(Probe, { shown: 5 })));
    now = 220;
    frames.shift()!(now);
    now = 230;
    frames.shift()!(now);
    expect(measure).toHaveBeenCalledWith("monocode:stream-visible", {
      start: 0,
      end: 230,
      detail: { queueMs: 10, commitMs: 0, revealMs: 200, presentationMs: 20 },
    });
    const hidden = { ...block, text: "hello again" };
    const hiddenEvent = { type: "message.delta", text: " again" } as const;
    noteStreamArrival(hiddenEvent);
    noteStreamApplied([block], [hidden], [hiddenEvent], false);
    expect(streamMeasurement(hidden)?.pending).toEqual([]);
    const queuedWhileHidden = {
      type: "message.delta",
      text: " unseen",
    } as const;
    noteStreamArrival(queuedWhileHidden, false);
    const reopened = { ...hidden, text: "hello again unseen" };
    noteStreamApplied([hidden], [reopened], [queuedWhileHidden], true);
    expect(streamMeasurement(reopened)?.pending ?? []).toEqual([]);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
