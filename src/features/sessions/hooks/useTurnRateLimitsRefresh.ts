import { useEffect, useRef } from "react";
import type { Session } from "../model/session";
import { scheduleRateLimitsAutoRefresh } from "../../providers/model/rateLimitsCache";
import type { RateLimitProvider } from "../../providers/model/rateLimits";

function isRateLimitHarness(harness: string): harness is RateLimitProvider {
  return (
    harness === "claude" ||
    harness === "codex" ||
    harness === "opencode" ||
    harness === "devin"
  );
}

/**
 * Automatically triggers a debounced rate-limit refresh whenever an agent turn
 * finishes (i.e. a session transitions from busy to not busy). Clustered
 * turns for the same provider and account share a single refresh request.
 */
export function useTurnRateLimitsRefresh(
  sessions: Session[],
  busySessionIds: ReadonlySet<string>,
): void {
  const previousBusyRef = useRef(busySessionIds);
  const busyIdentitiesRef = useRef<
    Map<string, { harness: RateLimitProvider; providerAccountId?: string }>
  >(new Map());

  // Track the provider identity for currently busy sessions before any in-flight switch
  for (const session of sessions) {
    if (busySessionIds.has(session.id) && isRateLimitHarness(session.harness)) {
      if (!busyIdentitiesRef.current.has(session.id)) {
        busyIdentitiesRef.current.set(session.id, {
          harness: session.harness,
          providerAccountId: session.providerAccountId,
        });
      }
    }
  }

  useEffect(() => {
    const previous = previousBusyRef.current;
    if (previous !== busySessionIds) {
      for (const id of previous) {
        if (!busySessionIds.has(id)) {
          const tracked = busyIdentitiesRef.current.get(id);
          busyIdentitiesRef.current.delete(id);

          const session = sessions.find((s) => s.id === id);
          const harness =
            tracked?.harness ??
            (session && isRateLimitHarness(session.harness)
              ? session.harness
              : undefined);
          const accountId = tracked
            ? tracked.providerAccountId
            : session?.providerAccountId;

          if (harness) {
            scheduleRateLimitsAutoRefresh(harness, accountId ?? "default");
          }
        }
      }
      previousBusyRef.current = busySessionIds;
    }
  }, [busySessionIds, sessions]);
}
