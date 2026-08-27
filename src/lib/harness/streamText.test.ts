import { describe, expect, it } from "vitest";
import {
  applyCollectedTextUpdate,
  classifyCompletedText,
  createCollectedTextState,
  selectCollectedText,
  type CollectedTextUpdate,
} from "./streamText";

describe("classifyCompletedText", () => {
  it("recognizes an already emitted completed value", () => {
    expect(
      classifyCompletedText({ streamed: "complete", completed: "complete" }),
    ).toEqual({ kind: "already-emitted" });
  });

  it("returns a completed-only fallback", () => {
    expect(
      classifyCompletedText({ streamed: "", completed: "complete" }),
    ).toEqual({ kind: "fallback", text: "complete" });
  });

  it("returns only the missing suffix", () => {
    expect(
      classifyCompletedText({ streamed: "hel", completed: "hello" }),
    ).toEqual({ kind: "extends", suffix: "lo" });
  });

  it.each([
    { streamed: "hello", completed: "hell" },
    { streamed: "help", completed: "hello" },
  ])(
    "rejects incompatible completed text: $streamed -> $completed",
    (input) => {
      expect(classifyCompletedText(input)).toEqual({ kind: "conflict" });
    },
  );
});

describe("collected text state", () => {
  it("prefers an authoritative completed value for the active scope", () => {
    let state = createCollectedTextState();
    state = applyCollectedTextUpdate(state, {
      kind: "delta",
      scopeId: "message-1",
      text: "hello\n\n",
    });
    state = applyCollectedTextUpdate(state, {
      kind: "completed",
      scopeId: "message-1",
      text: "hello",
    });

    expect(selectCollectedText(state)).toBe("hello");
  });

  it("uses exact streamed text without a completed value", () => {
    const state = applyCollectedTextUpdate(createCollectedTextState(), {
      kind: "delta",
      scopeId: "message-1",
      text: "bookkeeper\n\n",
    });

    expect(selectCollectedText(state)).toBe("bookkeeper\n\n");
  });

  it("does not let a late completion replace a newer native scope", () => {
    let state = createCollectedTextState();
    const updates: CollectedTextUpdate[] = [
      { kind: "delta", scopeId: "message-1", text: "old" },
      { kind: "delta", scopeId: "message-2", text: "new" },
      { kind: "completed", scopeId: "message-1", text: "older complete" },
    ];
    for (const update of updates) {
      state = applyCollectedTextUpdate(state, update);
    }

    expect(selectCollectedText(state)).toBe("new");
  });
});
