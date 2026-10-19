import { describe, expect, it } from "vitest";
import { inProjectScope } from "./projectScope";

describe("inProjectScope", () => {
  it("keeps everything without a scope", () => {
    expect(inProjectScope("/work/other", undefined)).toBe(true);
  });

  it("keeps the scoped project's items and drops other projects'", () => {
    expect(inProjectScope("/work/app", "/work/app")).toBe(true);
    expect(inProjectScope("/work/app/", "/work/app")).toBe(true);
    expect(inProjectScope("/work/other", "/work/app")).toBe(false);
  });

  it("keeps items filed under no project in every scope", () => {
    expect(inProjectScope(undefined, "/work/app")).toBe(true);
    expect(inProjectScope("", "/work/app")).toBe(true);
    expect(inProjectScope("~", "/work/app")).toBe(true);
  });
});
