import { useEffect, useState } from "react";
import {
  loadCommitMessage,
  peekCommitMessage,
  type CommitMessage,
} from "../model/commitMessage";

/**
 * Lazily loads a commit's full message. `undefined` means loading, `null`
 * means Git could not report it, otherwise the message. Callers mount this
 * only for a visible card, so the fetch stays hover-driven.
 */
export function useCommitMessage(
  cwd: string,
  sha: string,
): CommitMessage | null | undefined {
  const [message, setMessage] = useState<CommitMessage | null | undefined>(() =>
    peekCommitMessage(cwd, sha),
  );

  useEffect(() => {
    if (!cwd || cwd === "~" || !sha) {
      setMessage(undefined);
      return;
    }
    const cached = peekCommitMessage(cwd, sha);
    if (cached) {
      setMessage(cached);
      return;
    }
    let cancelled = false;
    setMessage(undefined);
    void loadCommitMessage(cwd, sha).then((value) => {
      if (!cancelled) setMessage(value);
    });
    return () => {
      cancelled = true;
    };
  }, [cwd, sha]);

  return message;
}
