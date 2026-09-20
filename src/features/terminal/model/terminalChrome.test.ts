import { describe, expect, it } from "vitest";
import { isOscColorQuery, oscColorReply } from "./terminalChrome";

describe("osc color query", () => {
  it("detects a palette request", () => {
    expect(isOscColorQuery("?")).toBe(true);
    expect(isOscColorQuery("rgb:0000/0000/0000")).toBe(false);
  });

  it("reports 16-bit rgb for a hex color", () => {
    expect(oscColorReply(11, "#141b1f")).toBe(
      "\x1b]11;rgb:1414/1b1b/1f1f\x1b\\",
    );
  });
});
