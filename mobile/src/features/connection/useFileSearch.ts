import { useEffect, useState } from "react";
import { useConnection } from "./ConnectionProvider";

/** A file whose path matched, relative to the working copy. */
export type FileMatch = { path: string; name: string };

/** How long typing pauses before the desktop is asked. */
const DEBOUNCE_MS = 150;

/**
 * Files in the working copy matching `query`, best first, ranked on the desktop
 * as its quick open ranks them. Empty while there is no query.
 */
export function useFileSearch(projectId: string, query: string, worktreePath?: string) {
  const { command } = useConnection();
  const wanted = query.trim();
  const scope = `${projectId}\n${worktreePath ?? ""}`;
  // The last answer and what it answered; anything else is still on its way.
  const [answer, setAnswer] = useState<{
    key: string;
    results: FileMatch[];
    error: string | null;
  } | null>(null);
  const key = `${scope}\n${wanted}`;

  useEffect(() => {
    if (!wanted) return;
    let disposed = false;
    const timer = setTimeout(() => {
      command<{ results: FileMatch[] }>("project.search", {
        projectId,
        query: wanted,
        ...(worktreePath ? { worktreePath } : {}),
      }).then(
        (value) => {
          if (!disposed) setAnswer({ key, results: value.results, error: null });
        },
        (reason) => {
          if (!disposed)
            setAnswer({
              key,
              results: [],
              error: reason instanceof Error ? reason.message : String(reason),
            });
        },
      );
    }, DEBOUNCE_MS);
    return () => {
      disposed = true;
      clearTimeout(timer);
    };
  }, [projectId, wanted, worktreePath, command, key]);

  const current = wanted && answer?.key === key ? answer : null;
  // Until the new answer comes, the last one for this working copy stays shown.
  const shown = wanted && answer?.key.startsWith(`${scope}\n`) ? answer : null;
  const results = shown?.results ?? [];
  const searching = !!wanted && !current;
  const error = current?.error ?? null;
  return { results, searching, error };
}
