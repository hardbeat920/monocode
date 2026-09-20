import { describe, expect, it } from "vitest";
import { isAtxHeadingLine } from "./markdownSource";

describe("isAtxHeadingLine", () => {
  it("matches ATX headings", () => {
    expect(isAtxHeadingLine("# Title")).toBe(true);
    expect(isAtxHeadingLine("## Agent OS – Project Overview")).toBe(true);
    expect(isAtxHeadingLine("### What exists today")).toBe(true);
    expect(isAtxHeadingLine("###### Deep")).toBe(true);
  });

  it("allows up to three leading spaces", () => {
    expect(isAtxHeadingLine("   ## Indented")).toBe(true);
    expect(isAtxHeadingLine("    ## Too deep")).toBe(false);
  });

  it("rejects hashes that are not headings", () => {
    expect(isAtxHeadingLine("Not a heading")).toBe(false);
    expect(isAtxHeadingLine("#hashtag")).toBe(false);
    expect(isAtxHeadingLine("Text # not a heading")).toBe(false);
    expect(isAtxHeadingLine("####### seven")).toBe(false);
  });
});
