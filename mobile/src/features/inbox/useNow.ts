import { useEffect, useState } from "react";

/** The time, refreshed each `ms` while `live`, so ages and groups move on. */
export function useNow(live = true, ms = 60_000) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!live) return;
    // Caught up as it goes live, if it has fallen behind, then each `ms`. A
    // screen just opened hasn't, and redrawing it now would only double the work.
    const first = setTimeout(
      () => setNow((shown) => (Date.now() - shown >= ms ? Date.now() : shown)),
      0,
    );
    const timer = setInterval(() => setNow(Date.now()), ms);
    return () => {
      clearTimeout(first);
      clearInterval(timer);
    };
  }, [live, ms]);
  return now;
}
