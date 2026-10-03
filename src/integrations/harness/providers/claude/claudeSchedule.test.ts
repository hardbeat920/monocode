import { expect, it } from "vitest";
import { nextClaudeCronFire } from "./claudeSchedule";

it("finds the next local fire across lists, ranges, and steps", () => {
  const now = new Date(2026, 9, 3, 12, 6).getTime();
  expect(nextClaudeCronFire("*/5 12-14 * * 0,6", now)).toBe(
    new Date(2026, 9, 3, 12, 10).getTime(),
  );
});

it("uses either constrained day field and accepts Sunday as seven", () => {
  const now = new Date(2026, 9, 3, 12).getTime();
  expect(nextClaudeCronFire("0 9 15 * 7", now)).toBe(
    new Date(2026, 9, 4, 9).getTime(),
  );
});

it("keeps a reminder scheduled for the next leap day", () => {
  const now = new Date(2025, 2, 1).getTime();
  expect(nextClaudeCronFire("0 9 29 2 *", now)).toBe(
    new Date(2028, 1, 29, 9).getTime(),
  );
});

it.each([
  "* * * *",
  "*/0 * * * *",
  "60 * * * *",
  "0 24 * * *",
  "0 9 * 13 *",
  "0 9 * * MON",
])("does not guess a fire time for invalid or unsupported cron %s", (cron) => {
  expect(nextClaudeCronFire(cron, Date.now())).toBeNull();
});
