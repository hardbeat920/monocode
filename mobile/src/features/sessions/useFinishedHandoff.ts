import { useEffect, useRef, useState } from "react";

import type { FinishedSession } from "./activityStore";

/** The working pill must clear before its finished card starts to enter. */
export const WORKING_PRESENCE_MS = 280;

export function useFinishedHandoff(
  workingCount: number,
  finished: FinishedSession[],
  hidden: boolean,
) {
  const [shown, setShown] = useState(finished);
  const wasWorking = useRef(workingCount > 0);
  const holdUntil = useRef(0);

  useEffect(() => {
    if (wasWorking.current && workingCount === 0 && !hidden)
      holdUntil.current = Date.now() + WORKING_PRESENCE_MS;
    wasWorking.current = workingCount > 0;
    if (workingCount > 0 || hidden) holdUntil.current = 0;

    const remaining = holdUntil.current - Date.now();
    if (remaining <= 0) {
      setShown(finished);
      return;
    }
    // Dismissals and restarts still take effect while a new card waits.
    const current = new Map(finished.map((entry) => [entry.session.id, entry]));
    setShown((previous) =>
      previous.flatMap((entry) => current.get(entry.session.id) ?? []),
    );
    const timer = setTimeout(() => {
      holdUntil.current = 0;
      setShown(finished);
    }, remaining);
    return () => clearTimeout(timer);
  }, [workingCount, finished, hidden]);

  return shown;
}
