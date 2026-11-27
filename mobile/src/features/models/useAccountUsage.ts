import { useCallback, useEffect, useRef, useState } from "react";

import { useConnection } from "@/features/connection/ConnectionProvider";

export type AccountUsageSnapshot = {
  status: {
    tone: "ready" | "low" | "exhausted" | "checking" | "unknown";
    label: string;
    detail: string | null;
  };
  updatedAt: number;
  stale: boolean;
  meters: {
    title: string;
    usedPercent: number;
    percent: number;
    label: string;
    reset: string;
  }[];
};
type UsageResponse = { accounts: Record<string, AccountUsageSnapshot> };

/** Usage loads independently so the model picker and account choices stay available. */
export function useAccountUsage(
  projectId: string | undefined,
  provider: string | undefined,
) {
  const { command, pairing } = useConnection();
  const desktop = pairing?.desktop.fingerprint;
  const enabled =
    !!projectId && (provider === "claude" || provider === "codex");
  const key = `${desktop}\n${projectId}\n${provider}`;
  const [state, setState] = useState<{
    key: string;
    accounts: UsageResponse["accounts"];
    error: string | null;
    refreshing: boolean;
  } | null>(null);
  const generation = useRef(0);
  const active = useRef<string | null>(null);
  const pending = useRef<{
    key: string;
    refresh: boolean;
    result: Promise<void>;
  } | null>(null);

  const load = useCallback(
    function fetchUsage(refresh: boolean): Promise<void> {
      if (!enabled || active.current !== key) return Promise.resolve();
      const running = pending.current;
      if (running?.key === key) {
        // A manual refresh during first load must still force a fresh probe.
        if (refresh && !running.refresh)
          return running.result.then(() => fetchUsage(true));
        return running.result;
      }
      const version = generation.current + 1;
      generation.current = version;
      setState((previous) => ({
        key,
        accounts: previous?.key === key ? previous.accounts : {},
        error: null,
        refreshing: true,
      }));
      const result = command<UsageResponse>("accounts.usage", {
        projectId,
        provider,
        ...(refresh ? { refresh: true } : {}),
      })
        .then(
          (response) => {
            if (generation.current !== version) return;
            setState({
              key,
              accounts: response.accounts,
              error: null,
              refreshing: false,
            });
          },
          (reason) => {
            if (generation.current !== version) return;
            const message =
              reason instanceof Error ? reason.message : String(reason);
            setState((previous) => ({
              key,
              accounts: previous?.key === key ? previous.accounts : {},
              error:
                message === "Unsupported mobile operation"
                  ? "Update the desktop app to show account usage."
                  : "Could not refresh usage. Try again.",
              refreshing: false,
            }));
          },
        )
        .finally(() => {
          if (pending.current?.result === result) pending.current = null;
        });
      pending.current = { key, refresh, result };
      return result;
    },
    [command, enabled, key, projectId, provider],
  );

  useEffect(() => {
    active.current = key;
    void load(false);
    return () => {
      active.current = null;
      pending.current = null;
      generation.current += 1;
    };
  }, [key, load]);

  const current = state?.key === key ? state : null;
  return {
    accounts: current?.accounts ?? {},
    error: current?.error ?? null,
    refreshing: current?.refreshing ?? enabled,
    refresh: useCallback(() => {
      void load(true);
    }, [load]),
  };
}
