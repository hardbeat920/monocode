import { useLayoutEffect, useState, type RefObject } from "react";

/**
 * Whether any `.truncate` text inside `ref` is cut off. Only measures while
 * `enabled`, and re-measures when `content` changes or the element resizes.
 */
export function useTruncated(
  ref: RefObject<HTMLElement | null>,
  enabled: boolean,
  content?: string,
): boolean {
  const [truncated, setTruncated] = useState(false);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!enabled || !el) {
      setTruncated(false);
      return;
    }
    const measure = () =>
      setTruncated(
        Array.from(el.querySelectorAll<HTMLElement>(".truncate")).some(
          (node) => node.scrollWidth > node.clientWidth + 1,
        ),
      );
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, [ref, enabled, content]);
  return truncated;
}
