import { useEffect, useRef, useState } from "react";

import type { SessionSummary } from "@/features/workspace/types";
import { useWorkspace } from "@/features/workspace/WorkspaceProvider";

type Pages = { key: string; sessions: SessionSummary[]; hasMore: boolean };

/** The pages last read for each project and search, so the list shows them
 *  at once when the drawer opens again while it reads them anew. */
const lastRead = new Map<string, Pages>();
const remember = (pages: Pages | null) => {
  if (pages) lastRead.set(pages.key, pages);
  return pages;
};

/**
 * A project's sessions from the desktop a page at a time, newest first: the
 * first page when the list opens or the search changes, the next as the list
 * nears its end. Null until the first page for this search lands.
 */
export function useSessionPages(projectId: string, query: string, active: boolean) {
  const { listSessions } = useWorkspace();
  const key = `${projectId}\n${query}`;
  const [pages, setPages] = useState<Pages | null>(() => lastRead.get(key) ?? null);
  const [loadingMore, setLoadingMore] = useState(false);
  const loading = useRef(false);
  const generation = useRef(0);
  const refreshing = useRef(false);

  useEffect(() => {
    const requestGeneration = ++generation.current;
    loading.current = false;
    if (!active) return;
    refreshing.current = true;
    // Typing waits for a pause; opening loads at once.
    const timer = setTimeout(
      () => {
        setLoadingMore(false);
        listSessions(projectId, query, 0).then(
          (page) => {
            if (generation.current !== requestGeneration) return;
            // The same first page leaves the list, and any pages after it, as they are.
            setPages((previous) =>
              generation.current !== requestGeneration ||
              (previous?.key === key && sameFirstPage(previous.sessions, page.sessions))
                ? previous
                : remember({ key, ...page }),
            );
          },
          () => {
            // An older desktop without paging: the poll's recent sessions stand in.
          },
        ).finally(() => {
          if (generation.current === requestGeneration) refreshing.current = false;
        });
      },
      query ? 250 : 0,
    );
    return () => {
      generation.current += 1;
      refreshing.current = false;
      clearTimeout(timer);
    };
  }, [active, key, projectId, query, listSessions]);

  const current = pages?.key === key ? pages : (lastRead.get(key) ?? null);
  if (!current) return null;

  const loadMore = () => {
    if (!active || !current.hasMore || loading.current || refreshing.current) return;
    const requestGeneration = generation.current;
    loading.current = true;
    setLoadingMore(true);
    listSessions(projectId, query, current.sessions.length)
      .then(
        (page) =>
          setPages((previous) => {
            if (generation.current !== requestGeneration || previous?.key !== key) return previous;
            // New sessions shift the offsets; skip any already listed.
            const seen = new Set(previous.sessions.map((session) => session.id));
            return remember({
              key,
              sessions: [...previous.sessions, ...page.sessions.filter((s) => !seen.has(s.id))],
              hasMore: page.hasMore,
            });
          }),
        () => {},
      )
      .finally(() => {
        if (generation.current !== requestGeneration) return;
        loading.current = false;
        setLoadingMore(false);
      });
  };

  return { sessions: current.sessions, hasMore: current.hasMore, loadMore, loadingMore: active && loadingMore };
}

function sameFirstPage(shown: SessionSummary[], page: SessionSummary[]) {
  return (
    page.length <= shown.length &&
    page.every((session, index) => JSON.stringify(session) === JSON.stringify(shown[index]))
  );
}

/**
 * What a session list shows: the paged sessions, kept fresh by the poll's copy
 * of any it carries and by renames and deletes made here, or, until the first
 * page lands, the poll's recent ones matching the search.
 */
export function visibleSessions(
  pages: ReturnType<typeof useSessionPages>,
  recent: SessionSummary[],
  query: string,
  edited: (session: SessionSummary) => SessionSummary | null,
) {
  const live = new Map(recent.map((session) => [session.id, session]));
  return pages
    ? pages.sessions.flatMap((session) => live.get(session.id) ?? edited(session) ?? [])
    : recent.filter((s) =>
        [s.title, s.model, s.branch].some((field) => field.toLowerCase().includes(query)),
      );
}
