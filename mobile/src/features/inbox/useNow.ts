import { useEffect, useState } from "react";

/** The time, refreshed each `ms` while `live`, so ages and groups move on. */
export function useNow(live = true, ms = 60_000) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!live) return;
    setNow(Date.now());
    const timer = setInterval(() => setNow(Date.now()), ms);
    return () => clearInterval(timer);
  }, [live, ms]);
  return now;
}
