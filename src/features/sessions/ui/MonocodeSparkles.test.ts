import { describe, expect, it } from "vitest";
import { shouldCelebrateMonocode } from "./MonocodeSparkles";

describe("MonoCode sparkles", () => {
  it("celebrates only a turn sent moments ago", () => {
    const now = 1_000_000;
    expect(shouldCelebrateMonocode("fresh", now - 500, now)).toBe(true);
    expect(shouldCelebrateMonocode("old", now - 60_000, now)).toBe(false);
    expect(shouldCelebrateMonocode("unknown", undefined, now)).toBe(false);
  });
});
