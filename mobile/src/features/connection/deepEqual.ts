/**
 * Whether two JSON-shaped values hold the same data. Builds no strings, and
 * stops at the first difference.
 */
export function deepEqual(a: unknown, b: unknown): boolean {
  if (a === b) return true;
  if (typeof a !== "object" || typeof b !== "object" || !a || !b) return false;
  if (Array.isArray(a)) {
    if (!Array.isArray(b) || a.length !== b.length) return false;
    for (let i = 0; i < a.length; i++) if (!deepEqual(a[i], b[i])) return false;
    return true;
  }
  if (Array.isArray(b)) return false;
  const left = a as Record<string, unknown>;
  const right = b as Record<string, unknown>;
  const keys = Object.keys(left);
  // Undefined fields never reach the wire, so they count as missing.
  const defined = (record: Record<string, unknown>) =>
    Object.keys(record).filter((key) => record[key] !== undefined);
  const other = defined(right);
  const own = keys.filter((key) => left[key] !== undefined);
  if (own.length !== other.length) return false;
  return own.every((key) => deepEqual(left[key], right[key]));
}
