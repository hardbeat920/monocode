import { describe, expect, it } from "vitest";
import { radioArrowStep } from "./radioKeys";

// Radio groups must be operable from the keyboard: arrows move between
// options and wrap, other keys are left alone (Tab still leaves the group).
describe("radioArrowStep", () => {
  const modes = ["Blueprint", "Development"] as const;

  it("moves forward and back with the arrow keys, wrapping", () => {
    expect(radioArrowStep("ArrowRight", modes, "Blueprint")).toBe(
      "Development",
    );
    expect(radioArrowStep("ArrowRight", modes, "Development")).toBe(
      "Blueprint",
    );
    expect(radioArrowStep("ArrowUp", modes, "Blueprint")).toBe("Development");
  });

  it("ignores other keys", () => {
    expect(radioArrowStep("Tab", modes, "Blueprint")).toBeNull();
    expect(radioArrowStep("Enter", modes, "Blueprint")).toBeNull();
  });
});
