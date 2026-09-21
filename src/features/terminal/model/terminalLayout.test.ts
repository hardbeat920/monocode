import { describe, expect, it } from "vitest";
import { terminalScrollbarWidth } from "./terminalLayout";

describe("terminalScrollbarWidth", () => {
  it("defaults to 14px when overview ruler width is unset", () => {
    expect(terminalScrollbarWidth(undefined)).toBe(14);
    expect(terminalScrollbarWidth({})).toBe(14);
  });

  it("honors an explicit overview ruler width", () => {
    expect(terminalScrollbarWidth({ width: 1 })).toBe(1);
    expect(terminalScrollbarWidth({ width: 0 })).toBe(0);
  });
});
