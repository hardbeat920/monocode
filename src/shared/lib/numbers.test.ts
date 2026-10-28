import { describe, expect, it } from "vitest";
import { formatInteger } from "./numbers";

describe("formatInteger", () => {
  it("groups thousands with commas", () => {
    expect(formatInteger(1250)).toBe("1,250");
    expect(formatInteger(4_222)).toBe("4,222");
    expect(formatInteger(253)).toBe("253");
  });
});
