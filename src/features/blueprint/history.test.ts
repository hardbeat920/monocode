import { describe, expect, it } from "vitest";
import { createHistory, record, redo, undo } from "./history";

// Undo must walk back exactly through what the user did, redo must replay
// it, and a new edit after undoing must discard the undone branch (otherwise
// redo would resurrect states the user moved away from).
describe("blueprint history", () => {
  it("undoes and redoes in order", () => {
    let h = record(record(createHistory("a"), "b"), "c");
    h = undo(h);
    expect(h.present).toBe("b");
    h = undo(h);
    expect(h.present).toBe("a");
    h = redo(h);
    expect(h.present).toBe("b");
  });

  it("drops the redo branch on a new edit", () => {
    let h = record(record(createHistory("a"), "b"), "c");
    h = record(undo(h), "x");
    expect(h.future).toEqual([]);
    expect(redo(h).present).toBe("x");
    expect(undo(h).present).toBe("b");
  });

  it("ignores no-op records and stops at the ends", () => {
    const h = record(createHistory("a"), "a");
    expect(h.past).toEqual([]);
    expect(undo(h)).toBe(h);
    expect(redo(h)).toBe(h);
  });

  it("keeps at most 100 steps", () => {
    let h = createHistory("0");
    for (let i = 1; i <= 150; i++) h = record(h, String(i));
    expect(h.past.length).toBe(100);
    expect(h.past[0]).toBe("50");
  });
});
