import { describe, expect, it } from "vitest";
import { isCompactCommand } from "./compact";

describe("compact command", () => {
  it("matches a standalone /compact command", () => {
    expect(isCompactCommand("/compact")).toBe(true);
    expect(isCompactCommand("  /COMPACT\n")).toBe(true);
  });

  it("does not consume ordinary prompt text", () => {
    expect(isCompactCommand("/compact now")).toBe(false);
    expect(isCompactCommand("mention /compact in docs")).toBe(false);
    expect(isCompactCommand("/compacted")).toBe(false);
  });
});
