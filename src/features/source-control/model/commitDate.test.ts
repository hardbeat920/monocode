import { describe, expect, it } from "vitest";
import { formatCommitTimestamp } from "./commitDate";

const MS = Date.parse("2026-09-28T13:45:00Z");
const TIMESTAMP = Math.floor(MS / 1000);

function absolute(ms: number): string {
  return new Intl.DateTimeFormat("en", {
    dateStyle: "long",
    timeStyle: "short",
  }).format(new Date(ms));
}

describe("formatCommitTimestamp", () => {
  it("leads with the relative age and appends the absolute time", () => {
    const now = MS + 5 * 3600 * 1000;
    expect(formatCommitTimestamp(TIMESTAMP, now, "en")).toBe(
      `5 hours ago (${absolute(MS)})`,
    );
  });

  it("rolls up to months for an older commit", () => {
    const now = MS + 40 * 24 * 3600 * 1000;
    expect(formatCommitTimestamp(TIMESTAMP, now, "en")).toBe(
      `last month (${absolute(MS)})`,
    );
  });

  it("returns empty for an unknown timestamp", () => {
    expect(formatCommitTimestamp(0)).toBe("");
    expect(formatCommitTimestamp(-1)).toBe("");
    expect(formatCommitTimestamp(Number.NaN)).toBe("");
    expect(formatCommitTimestamp(Number.POSITIVE_INFINITY)).toBe("");
  });
});
