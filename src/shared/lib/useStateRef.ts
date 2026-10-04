import { useCallback, useRef, useState, type RefObject } from "react";

/**
 * State whose ref tracks the latest requested value, not just the last
 * rendered one. Code that sets the value and then, in the same tick, compares
 * against the ref sees what it just asked for.
 */
export function useStateRef<T>(
  initial: T | (() => T),
): [T, (value: T) => void, RefObject<T>] {
  const [value, setState] = useState(initial);
  const ref = useRef(value);
  ref.current = value;
  const set = useCallback((next: T) => {
    ref.current = next;
    setState(next);
  }, []);
  return [value, set, ref];
}
