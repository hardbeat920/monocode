import {
  formatAbsoluteTime,
  formatRelativeTime,
} from "../../../shared/lib/time";

/**
 * "5 hours ago (Sep 28, 2026, 1:45 PM)" for a commit's author timestamp.
 * Git records whole unix seconds (`%at`); a non-finite or non-positive value
 * is treated as unknown so callers can drop the line.
 */
export function formatCommitTimestamp(
  timestampSeconds: number,
  now = Date.now(),
  locale?: string,
): string {
  if (!Number.isFinite(timestampSeconds) || timestampSeconds <= 0) return "";
  const ms = timestampSeconds * 1000;
  const relative = formatRelativeTime(ms, now, locale);
  const absolute = formatAbsoluteTime(ms, locale);
  if (!relative) return absolute;
  if (!absolute) return relative;
  return `${relative} (${absolute})`;
}
