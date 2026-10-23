import { describe, expect, it } from "vitest";

import type { Turn } from "@/features/workspace/types";
import { applyFrames } from "@/features/workspace/watchUpdates";

const turn = (id: string, text: string): Turn => ({
  id,
  items: [{ type: "text", id: `${id}:a`, text }],
});

describe("watch frames", () => {
  it("rejects append edits against a missing or stale baseline and accepts a fresh snapshot", () => {
    const patches = [
      {
        id: "a",
        edits: [{ path: ["items", 0, "text"], from: 4, append: "ing" }],
      },
    ];
    const frame = { order: ["a"], patches };
    expect(applyFrames([], [frame])).toBeNull();
    expect(applyFrames([turn("a", "wrong length")], [frame])).toBeNull();
    const restored = applyFrames(
      [],
      [{ order: ["a"], turns: [turn("a", "Work")] }, frame],
    );
    expect(restored?.turns).toEqual([turn("a", "Working")]);
  });

  it("swaps in changed turns and keeps the rest as they were", () => {
    const a = turn("a", "done");
    const start = [a, turn("b", "Wor")];
    const applied = applyFrames(start, [
      { busy: true, order: ["a", "b"], turns: [turn("b", "Work")] },
      { busy: true, order: ["a", "b"], turns: [turn("b", "Working")] },
    ])!;
    expect(applied.turns[0]).toBe(a);
    expect(applied.turns[1]).toEqual(turn("b", "Working"));
    expect(applied.busy).toBe(true);
  });

  it("keeps the same list when a frame changes nothing", () => {
    const start = [turn("a", "done")];
    expect(
      applyFrames(start, [{ busy: false, order: ["a"], turns: [] }])!.turns,
    ).toBe(start);
  });

  it("asks for a fresh watch when a frame names a turn it never got", () => {
    expect(applyFrames([], [{ order: ["a"], turns: [] }])).toBeNull();
  });

  it("carries an error until the next good frame", () => {
    expect(
      applyFrames([], [{ error: "Mono is no longer available" }])!.error,
    ).toBe("Mono is no longer available");
  });
});
