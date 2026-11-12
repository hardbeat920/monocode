import { useEffect, useState } from "react";
import {
  githubStatus,
  githubWorkItem,
  peekGithubWorkItem,
} from "../../inbox/model/githubTasks";
import type { GithubRef } from "../model/githubLinks";

/**
 * Enriches a GitHub reference badge with the real PR/issue title fetched
 * through the existing `gh` backend. Resolution is best-effort: when the CLI
 * is missing, unauthenticated, or the request fails, the badge keeps its
 * `#number` label. Commits have no work-item title, so they are left alone.
 */

const titleCache = new Map<string, string>();
// Avoid re-spawning `gh` for refs we already failed to resolve this session.
const failedKeys = new Set<string>();
let authProbe: Promise<boolean> | null = null;

function githubAuthenticated(): Promise<boolean> {
  if (!authProbe) {
    // `Promise.resolve().then` also traps a synchronous throw from `invoke`
    // when no Tauri runtime is present (tests, browser preview).
    authProbe = Promise.resolve()
      .then(() => githubStatus())
      .then((status) => status.authenticated)
      .catch(() => false);
  }
  return authProbe;
}

function cacheKey(repo: string, kind: string, number: string | number): string {
  return `${repo.toLowerCase()}:${kind}:${number}`;
}

export function useGithubRefTitle(
  ref: GithubRef | null,
  cwd: string | undefined,
): string | null {
  const repo = ref?.repo;
  const kind = ref?.kind;
  const label = ref?.label;

  const [title, setTitle] = useState<string | null>(() =>
    repo && kind && label
      ? (titleCache.get(cacheKey(repo, kind, label.replace(/^#/, ""))) ?? null)
      : null,
  );

  useEffect(() => {
    // Only pull requests and issues expose a work-item title via `gh`.
    if (!repo || (kind !== "pr" && kind !== "issue") || !label) {
      setTitle(null);
      return;
    }
    const number = Number(label.replace(/^#/, ""));
    if (!Number.isSafeInteger(number) || number <= 0) return;

    const key = cacheKey(repo, kind, number);
    const cached = titleCache.get(key);
    if (cached) {
      setTitle(cached);
      return;
    }
    setTitle(null);
    if (failedKeys.has(key)) return;

    // The Inbox may have already loaded this item.
    const peeked = peekGithubWorkItem(repo, kind, number);
    if (peeked?.title) {
      titleCache.set(key, peeked.title);
      setTitle(peeked.title);
      return;
    }

    let cancelled = false;
    void githubAuthenticated().then((ok) => {
      if (cancelled || !ok) return;
      githubWorkItem(cwd ?? "", repo, kind, number)
        .then((item) => {
          if (cancelled) return;
          titleCache.set(key, item.title);
          setTitle(item.title);
        })
        .catch(() => {
          failedKeys.add(key);
        });
    });
    return () => {
      cancelled = true;
    };
  }, [repo, kind, label, cwd]);

  return kind === "commit" ? null : title;
}
