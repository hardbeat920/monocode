import { describe, expect, it } from "vitest";
import { oscLinkNeedsConfirm } from "./terminalLinks";

describe("oscLinkNeedsConfirm", () => {
  it("opens a link that shows its own target", () => {
    expect(oscLinkNeedsConfirm("https://a.test/x", "https://a.test/x")).toBe(
      false,
    );
    expect(oscLinkNeedsConfirm(" https://a.test/x ", "https://a.test/x")).toBe(
      false,
    );
  });

  it("asks when the visible text differs from the target", () => {
    expect(oscLinkNeedsConfirm("docs", "https://a.test/x")).toBe(true);
    expect(oscLinkNeedsConfirm("https://a.test/x", "https://evil.test/")).toBe(
      true,
    );
    // Only part of a wrapped link is on this row.
    expect(oscLinkNeedsConfirm("https://a.test/", "https://a.test/x")).toBe(
      true,
    );
  });
});
