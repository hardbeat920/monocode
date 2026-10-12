const RELATIVE_DIVISIONS: [number, Intl.RelativeTimeFormatUnit][] = [
  [60, "second"],
  [60, "minute"],
  [24, "hour"],
  [7, "day"],
  [4.34524, "week"],
  [12, "month"],
  [Number.POSITIVE_INFINITY, "year"],
];

/**
 * "2 hours ago" from an epoch-milliseconds instant. Returns empty for a
 * non-finite value or an environment without `Intl.RelativeTimeFormat`.
 */
export function formatRelativeTime(
  ms: number,
  now = Date.now(),
  locale?: string,
): string {
  if (!Number.isFinite(ms)) return "";
  const delta = Math.round((ms - now) / 1000);
  const abs = Math.abs(delta);
  let value = delta;
  let unit: Intl.RelativeTimeFormatUnit = "second";
  let amount = abs;
  for (const [step, next] of RELATIVE_DIVISIONS) {
    unit = next;
    if (amount < step) break;
    value = Math.round(value / step);
    amount = Math.abs(value);
  }
  try {
    return new Intl.RelativeTimeFormat(locale, { numeric: "auto" }).format(
      value,
      unit,
    );
  } catch {
    return "";
  }
}

/** "September 28, 2026, 1:45 PM" from an epoch-milliseconds instant. */
export function formatAbsoluteTime(ms: number, locale?: string): string {
  if (!Number.isFinite(ms)) return "";
  try {
    return new Intl.DateTimeFormat(locale, {
      dateStyle: "long",
      timeStyle: "short",
    }).format(new Date(ms));
  } catch {
    return "";
  }
}
