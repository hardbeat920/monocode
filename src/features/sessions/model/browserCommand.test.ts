import { describe, expect, it } from "vitest";
import { browserGrantChange, consumeBrowserCommand } from "./browserCommand";
import { consumeOperatorCommand } from "./operatorCommand";

describe("/browser", () => {
  it("takes a leading command and leaves the prompt", () => {
    expect(consumeBrowserCommand("/browser open example.com")).toEqual({
      text: "open example.com",
      matched: true,
    });
    expect(consumeBrowserCommand("  /BROWSER\nlook it up")).toEqual({
      text: "look it up",
      matched: true,
    });
    expect(consumeBrowserCommand("/browser")).toEqual({
      text: "",
      matched: true,
    });
  });

  it("ignores the word elsewhere and longer commands", () => {
    for (const text of ["use /browser", "/browsers", "/browser-extra x"])
      expect(consumeBrowserCommand(text)).toEqual({ text, matched: false });
  });

  it("keeps the Operator prefix in front of it for the operator parser", () => {
    const browser = consumeBrowserCommand("/operator /browser list tabs");
    expect(browser).toEqual({ text: "/operator list tabs", matched: true });
    expect(consumeOperatorCommand(browser.text)).toEqual({
      text: "list tabs",
      matched: true,
    });
  });
});

describe("/browser grant", () => {
  it("switches when alone", () => {
    expect(browserGrantChange(false, true)).toBe(true);
    expect(browserGrantChange(true, true)).toBe(false);
  });

  it("with a prompt grants once and leaves an existing grant alone", () => {
    expect(browserGrantChange(false, false)).toBe(true);
    expect(browserGrantChange(true, false)).toBeNull();
  });
});
