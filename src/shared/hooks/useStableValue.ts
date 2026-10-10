import { useRef } from "react";

/** Structural equality for plain data: primitives, arrays, objects, Maps and Sets. */
export function plainEqual(a: unknown, b: unknown): boolean {
  if (Object.is(a, b)) return true;
  if (
    typeof a !== "object" ||
    typeof b !== "object" ||
    a === null ||
    b === null
  ) {
    return false;
  }
  if (Array.isArray(a)) {
    if (!Array.isArray(b) || a.length !== b.length) return false;
    return a.every((item, i) => plainEqual(item, b[i]));
  }
  if (a instanceof Map) {
    if (!(b instanceof Map) || a.size !== b.size) return false;
    for (const [key, value] of a) {
      if (!b.has(key) || !plainEqual(value, b.get(key))) return false;
    }
    return true;
  }
  if (a instanceof Set) {
    if (!(b instanceof Set) || a.size !== b.size) return false;
    for (const value of a) if (!b.has(value)) return false;
    return true;
  }
  if (Array.isArray(b) || b instanceof Map || b instanceof Set) return false;
  const left = a as Record<string, unknown>;
  const right = b as Record<string, unknown>;
  const keys = Object.keys(left);
  if (keys.length !== Object.keys(right).length) return false;
  return keys.every((key) => key in right && plainEqual(left[key], right[key]));
}

/**
 * The previous value while `equal` says nothing changed, so values derived
 * from fast-changing state (streaming sessions) keep their identity and
 * memoized consumers can skip the render.
 */
export function useStableValue<T>(
  value: T,
  equal: (previous: T, next: T) => boolean = plainEqual,
): T {
  const ref = useRef(value);
  if (ref.current !== value && !equal(ref.current, value)) ref.current = value;
  return ref.current;
}
