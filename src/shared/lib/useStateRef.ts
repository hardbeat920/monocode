import {
  useCallback,
  useLayoutEffect,
  useRef,
  useState,
  type RefObject,
} from "react";

/**
 * State paired with a ref holding the latest committed or explicitly
 * requested value. `set` updates the ref immediately so same-tick callers see
 * what they just asked for; the layout effect syncs it on commit, so a render
 * React discards never leaks into the ref.
 */
export function useStateRef<T>(
  initial: T | (() => T),
): [T, (value: T) => void, RefObject<T>] {
  const [value, setState] = useState(initial);
  const ref = useRef(value);
  useLayoutEffect(() => {
    ref.current = value;
  }, [value]);
  const set = useCallback((next: T) => {
    ref.current = next;
    setState(next);
  }, []);
  return [value, set, ref];
}
