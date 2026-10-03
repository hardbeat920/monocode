/** Next local-time fire for the five-field cron expressions Claude accepts. */
export function nextClaudeCronFire(cron: string, now: number): number | null {
  const parts = cron.trim().split(/\s+/);
  if (parts.length !== 5) return null;
  const bounds = [
    [0, 59],
    [0, 23],
    [1, 31],
    [1, 12],
    [0, 7],
  ];
  const fields = parts.map((part, index) =>
    cronValues(part, ...(bounds[index] as [number, number])),
  );
  if (fields.some((field) => field === null)) return null;
  const [minutes, hours, days, months, weekdays] = fields as Set<number>[];
  const date = new Date(now);
  date.setHours(12, 0, 0, 0);
  // Eight years include the next leap day even across a non-leap century.
  for (
    let offset = 0;
    offset < 8 * 366;
    offset++, date.setDate(date.getDate() + 1)
  ) {
    if (!months.has(date.getMonth() + 1)) continue;
    const dayMatches = days.has(date.getDate());
    const weekdayMatches =
      weekdays.has(date.getDay()) || (date.getDay() === 0 && weekdays.has(7));
    const matches =
      parts[2] === "*"
        ? weekdayMatches
        : parts[4] === "*"
          ? dayMatches
          : dayMatches || weekdayMatches;
    if (!matches) continue;
    for (const hour of [...hours].sort((a, b) => a - b)) {
      for (const minute of [...minutes].sort((a, b) => a - b)) {
        const candidate = new Date(
          date.getFullYear(),
          date.getMonth(),
          date.getDate(),
          hour,
          minute,
        );
        if (candidate.getHours() !== hour || candidate.getMinutes() !== minute)
          continue;
        if (candidate.getTime() > now) return candidate.getTime();
      }
    }
  }
  return null;
}

function cronValues(
  part: string,
  min: number,
  max: number,
): Set<number> | null {
  const values = new Set<number>();
  for (const entry of part.split(",")) {
    const match = /^(\*|\d+(?:-\d+)?)(?:\/(\d+))?$/.exec(entry);
    if (!match) return null;
    const step = Number(match[2] ?? 1);
    const range =
      match[1] === "*" ? [min, max] : match[1].split("-").map(Number);
    const start = range[0];
    const end = range[1] ?? (match[2] ? max : start);
    if (
      !Number.isSafeInteger(step) ||
      step < 1 ||
      start < min ||
      end > max ||
      start > end
    )
      return null;
    for (let value = start; value <= end; value += step) values.add(value);
  }
  return values.size ? values : null;
}
