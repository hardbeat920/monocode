import { describe, expect, it } from "vitest";
import {
  orderedSessionActionIds,
  pruneSessionSelection,
  toggleSessionSelection,
} from "./sessionSelection";

describe("session selection", () => {
  it("toggles cards without mutating the current selection", () => {
    const selected = new Set(["one"]);
    const added = toggleSessionSelection(selected, "two");
    const removed = toggleSessionSelection(added, "one");

    expect([...selected]).toEqual(["one"]);
    expect([...added]).toEqual(["one", "two"]);
    expect([...removed]).toEqual(["two"]);
  });

  it("uses the whole selection when its card opens the action menu", () => {
    const selected = new Set(["three", "one"]);

    expect(
      orderedSessionActionIds("one", selected, ["one", "two", "three"]),
    ).toEqual(["one", "three"]);
  });

  it("uses only an unselected card when it opens the action menu", () => {
    const selected = new Set(["one", "three"]);

    expect(
      orderedSessionActionIds("two", selected, ["one", "two", "three"]),
    ).toEqual(["two"]);
  });

  it("drops selections that are no longer available", () => {
    expect([
      ...pruneSessionSelection(new Set(["one", "two"]), new Set(["two"])),
    ]).toEqual(["two"]);
  });
});
