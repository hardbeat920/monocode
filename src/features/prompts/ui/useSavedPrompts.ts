import { useCallback, useEffect, useState } from "react";
import {
  listSavedPrompts,
  subscribeSavedPrompts,
  type SavedPrompt,
} from "../model/savedPrompts";

type State = {
  prompts: SavedPrompt[] | null;
  error: string | null;
};

/** One list per window, shared by every composer and the Settings page. */
let cached: SavedPrompt[] | null = null;

/**
 * Saved prompts for this window, refreshed whenever any window saves or
 * deletes one. `prompts` is null until the first load finishes. With
 * `enabled` false nothing loads yet (composers wait for the first `!`).
 */
export function useSavedPrompts(
  enabled = true,
): State & { reload: () => void } {
  const [state, setState] = useState<State>({ prompts: cached, error: null });
  const [revision, setRevision] = useState(0);
  const reload = useCallback(() => setRevision((value) => value + 1), []);

  useEffect(
    () => (enabled ? subscribeSavedPrompts(reload) : undefined),
    [enabled, reload],
  );

  useEffect(() => {
    if (!enabled) return;
    let cancelled = false;
    listSavedPrompts()
      .then((prompts) => {
        cached = prompts;
        if (!cancelled) setState({ prompts, error: null });
      })
      .catch((error: unknown) => {
        if (!cancelled)
          setState((current) => ({
            prompts: current.prompts,
            error: error instanceof Error ? error.message : String(error),
          }));
      });
    return () => {
      cancelled = true;
    };
  }, [enabled, revision]);

  return { ...state, reload };
}
