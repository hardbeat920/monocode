import { useCallback, useEffect, useRef, useState } from "react";
import { fetchGithubPrChecks, type GithubPrChecks } from "../model/githubPrChecks";

const POLL_MS = 30_000;

export type GithubPrChecksView = {
  checks: GithubPrChecks | null;
  /** Initial load: no results yet, so the view must not read as "no checks". */
  loading: boolean;
  /** Revalidation of already-shown results. */
  refreshing: boolean;
  error: string | null;
  /** Previous results stayed on screen after a failed refresh. */
  stale: boolean;
  refresh: () => void;
};

/**
 * Loads PR checks on mount regardless of the active tab, keeps them fresh for
 * open PRs while mounted and visible, and allows initial and manual loads only
 * for closed or merged ones. Results are replaced together with headOid, never
 * merged across commits, and a late answer cannot outlive a PR change, a
 * revision change, or unmount.
 */
export function useGithubPrChecks(params: {
  cwd: string;
  repo: string;
  number: number;
  enabled: boolean;
  /** Open PRs poll; closed or merged ones load once and on demand. */
  open: boolean;
  poll?: boolean;
  revision?: number;
}): GithubPrChecksView {
  const { cwd, repo, number, enabled, open, poll = true, revision = 0 } = params;
  const [checks, setChecks] = useState<GithubPrChecks | null>(null);
  const [loading, setLoading] = useState(enabled);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [stale, setStale] = useState(false);
  const [manualTick, setManualTick] = useState(0);

  const mountedRef = useRef(true);
  const hasDataRef = useRef(false);
  const identityRef = useRef<string | null>(null);
  const epochRef = useRef(0);
  const flightRef = useRef<{ epoch: number } | null>(null);
  /** Trigger waiting behind an in-flight request; polls never outrank manual loads. */
  const queuedRef = useRef<"manual" | "auto" | null>(null);
  const openRef = useRef(open);
  openRef.current = open;
  const pollRef = useRef(poll);
  pollRef.current = poll;
  const previousPollRef = useRef(poll);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  const refresh = useCallback(() => setManualTick((tick) => tick + 1), []);

  const run = useCallback((mode?: "auto") => {
    const automatic = mode === "auto";
    // One request per PR revision; a trigger landing while it runs coalesces
    // into a single follow-up instead of being dropped or overlapped. A manual
    // load always outranks a queued poll tick.
    const flight = flightRef.current;
    if (flight && flight.epoch === epochRef.current) {
      if (!automatic || queuedRef.current == null) {
        queuedRef.current = automatic ? "auto" : "manual";
      }
      return;
    }
    queuedRef.current = null;
    const ticket = { epoch: epochRef.current };
    flightRef.current = ticket;
    const initial = !hasDataRef.current;
    if (initial) setLoading(true);
    else setRefreshing(true);
    fetchGithubPrChecks(cwd, repo, number)
      .then((next) => {
        if (!mountedRef.current || ticket.epoch !== epochRef.current) return;
        hasDataRef.current = true;
        // headOid and checks land together, so two commits never mix.
        setChecks(next);
        setError(null);
        setStale(false);
      })
      .catch((reason: unknown) => {
        if (!mountedRef.current || ticket.epoch !== epochRef.current) return;
        setError(reason instanceof Error ? reason.message : String(reason));
        if (hasDataRef.current) setStale(true);
      })
      .finally(() => {
        if (flightRef.current !== ticket) return;
        flightRef.current = null;
        if (!mountedRef.current || ticket.epoch !== epochRef.current) return;
        setLoading(false);
        setRefreshing(false);
        const queued = queuedRef.current;
        if (!queued) return;
        queuedRef.current = null;
        // An automatic follow-up waits for an open PR and a visible document;
        // visibilitychange or the next tick picks the work back up.
        if (
          queued === "auto" &&
          (document.hidden || !openRef.current || !pollRef.current)
        )
          return;
        runRef.current(queued === "auto" ? "auto" : undefined);
      });
  }, [cwd, repo, number]);
  const runRef = useRef(run);
  runRef.current = run;

  useEffect(() => {
    if (!enabled) {
      epochRef.current += 1;
      flightRef.current = null;
      queuedRef.current = null;
      identityRef.current = null;
      hasDataRef.current = false;
      setChecks(null);
      setLoading(false);
      setRefreshing(false);
      setError(null);
      setStale(false);
      return;
    }
    const identity = `${cwd}\u0000${repo}\u0000${number}`;
    if (identityRef.current !== identity) {
      // A different PR: drop the in-flight answer, any queue, and saved results.
      epochRef.current += 1;
      identityRef.current = identity;
      queuedRef.current = null;
      hasDataRef.current = false;
      setChecks(null);
      setError(null);
      setStale(false);
    }
    runRef.current();
  }, [cwd, repo, number, enabled, revision, manualTick]);

  useEffect(() => {
    const resumed = poll && !previousPollRef.current;
    previousPollRef.current = poll;
    if (!enabled || !open || !poll) return;
    if (resumed && !document.hidden) runRef.current("auto");
    const timer = window.setInterval(() => {
      if (document.hidden) return;
      runRef.current("auto");
    }, POLL_MS);
    const onVisibility = () => {
      if (!document.hidden) runRef.current("auto");
    };
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      window.clearInterval(timer);
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [enabled, open, poll]);

  return { checks, loading, refreshing, error, stale, refresh };
}
