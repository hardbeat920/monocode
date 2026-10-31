import { useFocusEffect } from "expo-router";
import { useCallback, useState } from "react";

import { useConnection } from "@/features/connection/ConnectionProvider";

/** A Mono's details, as the desktop's Overview tab has them (mono.profile). */
export type MonoProfile = {
  /** What it's doing now, e.g. "Working on Fix the composer" or "Idle · last active 2h ago". */
  status: string;
  state: "idle" | "working" | "needs-you";
  contributions: {
    total: number;
    streak: number;
    /** Day by day from a Sunday 26 weeks back to today, each 0 to 4. */
    levels: number[];
  };
  today: {
    merged: number;
    additions: number;
    deletions: number;
    sessions: number;
    habitsRan: number;
    habitsTotal: number;
  };
  upNext: { id: string; name: string; schedule: string; when: string; running: boolean }[];
  habits: MonoHabit[];
  documents: { id: string; title: string; kind: string; summary: string; updatedAt: number }[];
  /** What its look can be: the desktop's swatches, as `#rrggbb`, and mascots. */
  colors: string[];
  mascots: string[];
};

/** A habit whole, for its page: what it's told to do, when, and how its
 *  latest runs went, newest first. */
export type MonoHabit = {
  id: string;
  name: string;
  instructions: string;
  schedule: string;
  enabled: boolean;
  when: string;
  running: boolean;
  lastRunAt?: number | null;
  lastOutcome?: string | null;
  lastError?: string | null;
  runs: { at: number; durationMs: number; outcome: string; report?: string | null; error?: string | null }[];
};

/** The profile, read each time its screen comes into view, and on a pull. */
export function useMonoProfile(monoId: string) {
  const { command } = useConnection();
  const [profile, setProfile] = useState<MonoProfile | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const load = useCallback(
    async (pulled = false) => {
      if (pulled) setRefreshing(true);
      try {
        setProfile(await command<MonoProfile>("mono.profile", { monoId }));
        setError(null);
      } catch (reason) {
        setError(reason instanceof Error ? reason.message : String(reason));
      } finally {
        if (pulled) setRefreshing(false);
      }
    },
    [command, monoId],
  );
  useFocusEffect(
    useCallback(() => {
      void load();
    }, [load]),
  );
  return { profile, error, refreshing, refresh: () => void load(true) };
}
