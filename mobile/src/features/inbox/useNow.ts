import { useEffect, useState } from "react";

/** The time, refreshed each `ms` while `live`, so ages and groups move on. */
export function useNow(live = true, ms = 60_000) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!live) return;
    // Caught up as it goes live, then each `ms`.
    const first = setTimeout(() => setNow(Date.now()), 0);
    const timer = setInterval(() => setNow(Date.now()), ms);
    return () => {
      clearTimeout(first);
      clearInterval(timer);
    };
  }, [live, ms]);
  return now;
}
