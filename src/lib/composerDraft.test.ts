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

describe("composerDraft", () => {
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

  it("swallows write failures", async () => {
    invoke.mockRejectedValueOnce(new Error("db locked"));
    saveSessionDraft("s-4", "text");
    await vi.advanceTimersByTimeAsync(600);
    expect(invoke).toHaveBeenCalledTimes(1);
  });

  it("loads a draft and treats failures as empty", async () => {
    invoke.mockResolvedValueOnce("restored draft");
    await expect(loadSessionDraft("s-5")).resolves.toBe("restored draft");
    invoke.mockRejectedValueOnce(new Error("missing table"));
    await expect(loadSessionDraft("s-6")).resolves.toBe("");
  });
});
