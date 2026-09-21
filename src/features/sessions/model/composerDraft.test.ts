// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

import {
  discardPendingDraft,
  flushSessionDraft,
  loadSessionDraft,
  saveSessionDraft,
} from "./composerDraft";

/**
 * Behavior tests for the per-session composer draft store: debounced saves
 * keyed by session, flush/retry semantics on failure, discard scoping, and
 * draft loads that degrade to empty.
 */
describe("composerDraft", () => {
  /** Default invoke mock: successful no-op writes, reset per test. */
  beforeEach(() => {
    vi.useFakeTimers();
    invoke.mockReset();
    // flushDraft chains `.catch` on the invoke result, so the default mock
    // must resolve (Once-stubs registered per test still take priority).
    invoke.mockResolvedValue(undefined);
  });

  afterEach(() => {
    discardPendingDraft();
    vi.useRealTimers();
  });

  it("collapses rapid saves into one debounced invoke", async () => {
    saveSessionDraft("s-1", "a");
    saveSessionDraft("s-1", "ab");
    saveSessionDraft("s-1", "abc");
    expect(invoke).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(600);
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith("composer_draft_set", {
      sessionId: "s-1",
      text: "abc",
    });
  });

  it("persists sessions independently in split view", async () => {
    saveSessionDraft("s-a", "draft for A");
    saveSessionDraft("s-b", "draft for B");
    await vi.advanceTimersByTimeAsync(600);
    expect(invoke).toHaveBeenCalledTimes(2);
    expect(invoke).toHaveBeenCalledWith("composer_draft_set", {
      sessionId: "s-a",
      text: "draft for A",
    });
    expect(invoke).toHaveBeenCalledWith("composer_draft_set", {
      sessionId: "s-b",
      text: "draft for B",
    });
  });

  it("a second pane's save does not clobber the first session's pending draft", async () => {
    saveSessionDraft("s-a", "A text");
    saveSessionDraft("s-b", "B text");
    await flushSessionDraft();
    expect(invoke).toHaveBeenCalledTimes(2);
    expect(invoke).toHaveBeenCalledWith("composer_draft_set", {
      sessionId: "s-a",
      text: "A text",
    });
    expect(invoke).toHaveBeenCalledWith("composer_draft_set", {
      sessionId: "s-b",
      text: "B text",
    });
  });

  it("flushes immediately on flushSessionDraft", async () => {
    saveSessionDraft("s-2", "pending text");
    await flushSessionDraft();
    expect(invoke).toHaveBeenCalledWith("composer_draft_set", {
      sessionId: "s-2",
      text: "pending text",
    });
  });

  it("discardPendingDraft drops the pending write", () => {
    saveSessionDraft("s-3", "never saved");
    discardPendingDraft();
    vi.advanceTimersByTime(10_000);
    expect(invoke).not.toHaveBeenCalled();
  });

  it("timer-fired write failures are swallowed silently", async () => {
    invoke.mockRejectedValueOnce(new Error("db locked"));
    saveSessionDraft("s-4", "text");
    await vi.advanceTimersByTimeAsync(600);
    expect(invoke).toHaveBeenCalledTimes(1);
  });

  it("flushSessionDraft re-pends failed writes, rejects only if still unwritten", async () => {
    invoke
      .mockRejectedValueOnce(new Error("db locked"))
      .mockResolvedValueOnce(undefined);
    saveSessionDraft("s-4", "text");
    await expect(flushSessionDraft()).resolves.toBeUndefined();
    // First write failed, retry on the next pass succeeded.
    expect(invoke).toHaveBeenCalledTimes(2);
    expect(invoke).toHaveBeenNthCalledWith(2, "composer_draft_set", {
      sessionId: "s-4",
      text: "text",
    });
  });

  it("flushSessionDraft rejects when a write keeps failing", async () => {
    invoke.mockRejectedValue(new Error("db locked"));
    saveSessionDraft("s-5", "text");
    await expect(flushSessionDraft()).rejects.toThrow("composer_draft_set failed");
  });

  it("flushSessionDraft drains saves added while a write is in flight", async () => {
    let releaseFirst: (() => void) | undefined;
    invoke.mockImplementationOnce(
      () =>
        new Promise<void>((resolve) => {
          releaseFirst = resolve;
        }),
    );
    saveSessionDraft("s-6", "first");
    const flush = flushSessionDraft();
    // While the first write hangs, the pane saves again.
    saveSessionDraft("s-6", "second");
    await vi.advanceTimersByTimeAsync(600);
    releaseFirst?.();
    await expect(flush).resolves.toBeUndefined();
    expect(invoke).toHaveBeenCalledWith("composer_draft_set", {
      sessionId: "s-6",
      text: "first",
    });
    expect(invoke).toHaveBeenCalledWith("composer_draft_set", {
      sessionId: "s-6",
      text: "second",
    });
  });

  it("loads a draft and treats failures as empty", async () => {
    invoke.mockResolvedValueOnce("restored draft");
    await expect(loadSessionDraft("s-5")).resolves.toBe("restored draft");
    invoke.mockRejectedValueOnce(new Error("missing table"));
    await expect(loadSessionDraft("s-6")).resolves.toBe("");
  });
});
