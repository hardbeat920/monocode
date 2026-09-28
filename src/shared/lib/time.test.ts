import { describe, expect, it } from "vitest";
import { formatAbsoluteTime, formatRelativeTime } from "./time";

describe("formatRelativeTime", () => {
  const now = Date.parse("2026-08-27T12:00:00Z");

  it("formats hours ago", () => {
    expect(
      formatRelativeTime(Date.parse("2026-08-27T10:00:00Z"), now, "en"),
    ).toBe("2 hours ago");
  });

  it("returns empty for a non-finite instant", () => {
    expect(formatRelativeTime(Number.NaN)).toBe("");
    expect(formatRelativeTime(Number.POSITIVE_INFINITY)).toBe("");
  });
});

describe("formatAbsoluteTime", () => {
  it("formats the date and time", () => {
    const ms = Date.parse("2026-09-28T13:45:00Z");
    const expected = new Intl.DateTimeFormat("en", {
      dateStyle: "long",
      timeStyle: "short",
    }).format(new Date(ms));
    expect(formatAbsoluteTime(ms, "en")).toBe(expected);
  });

  it("returns empty for a non-finite instant", () => {
    expect(formatAbsoluteTime(Number.NaN)).toBe("");
  });
});
