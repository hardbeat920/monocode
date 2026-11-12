import type { SessionSummary } from "@/features/workspace/types";

const DAY_MS = 86_400_000;

/** A local calendar day as a number of days, so daylight saving can't skew a difference. */
function dayNumber(ms: number) {
  const date = new Date(ms);
  return Date.UTC(date.getFullYear(), date.getMonth(), date.getDate()) / DAY_MS;
}

/** The desktop sidebar's heading for a session last active at `updatedAt`. */
export function ageLabel(updatedAt: number, now: number) {
  const days = dayNumber(now) - dayNumber(updatedAt);
  if (days <= 0) return "Today";
  if (days === 1) return "Yesterday";
  return days <= 6 ? "Last 7 days" : "Older";
}

/** The sessions that start a heading in a newest-first list, with its label. */
export function dayHeadings(sessions: SessionSummary[], now = Date.now()) {
  const headings = new Map<string, string>();
  let previous: string | undefined;
  for (const session of sessions) {
    const label = ageLabel(session.updatedAt, now);
    if (label !== previous) headings.set(session.id, label);
    previous = label;
  }
  return headings;
}
