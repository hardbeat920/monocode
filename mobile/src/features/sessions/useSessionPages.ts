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
 *
 * `reload` names what the pages follow, the sessions the poll carries: the
 * first page is read again when it changes, so a session archived or deleted
 * on the desktop leaves.
 */
export function useSessionPages(projectId: string, query: string, active: boolean, reload = "") {
  const { listSessions } = useWorkspace();
  const key = `${projectId}\n${query}`;
  const [pages, setPages] = useState<Pages | null>(() => lastRead.get(key) ?? null);
  const [loadingMore, setLoadingMore] = useState(false);
  const loading = useRef(false);

  useEffect(() => {
    if (!active) return;
    let cancelled = false;
    // Typing waits for a pause; opening loads at once.
    const timer = setTimeout(
      () => {
        listSessions(projectId, query, 0).then(
          (page) => {
            if (cancelled) return;
            // The same first page leaves the list, and any pages after it, as they are.
            setPages((previous) =>
              previous?.key === key && sameFirstPage(previous.sessions, page.sessions)
                ? previous
                : remember({ key, ...refreshedPages(previous?.key === key ? previous : null, page) }),
            );
          },
          () => {
            // An older desktop without paging: the poll's recent sessions stand in.
          },
        );
      },
      query ? 250 : 0,
    );
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [active, key, projectId, query, listSessions, reload]);

  const current = pages?.key === key ? pages : (lastRead.get(key) ?? null);
  if (!current) return null;

  const loadMore = () => {
    if (!current.hasMore || loading.current) return;
    loading.current = true;
    setLoadingMore(true);
    listSessions(projectId, query, current.sessions.length)
      .then(
        (page) =>
          setPages((previous) => {
            if (previous?.key !== key) return previous;
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
        loading.current = false;
        setLoadingMore(false);
      });
  };

  return { sessions: current.sessions, hasMore: current.hasMore, loadMore, loadingMore };
}

function sameFirstPage(shown: SessionSummary[], page: SessionSummary[]) {
  return (
    page.length <= shown.length &&
    page.every((session, index) => JSON.stringify(session) === JSON.stringify(shown[index]))
  );
}

/**
 * A first page read again, over what the list had. The rows past it that are
 * older than its last stay, so a list scrolled far down doesn't collapse; a
 * newer one missing from it has left the list.
 */
export function refreshedPages(
  previous: Pick<Pages, "sessions" | "hasMore"> | null,
  page: Omit<Pages, "key">,
): Omit<Pages, "key"> {
  const last = page.sessions.at(-1);
  if (!previous || !last || !page.hasMore) return page;
  const read = new Set(page.sessions.map((session) => session.id));
  const older = previous.sessions.filter(
    (session) => !read.has(session.id) && session.updatedAt < last.updatedAt,
  );
  return older.length
    ? { ...page, sessions: [...page.sessions, ...older], hasMore: previous.hasMore }
    : page;
}

/**
 * What a session list shows, newest first, as the desktop's sidebar does. The
 * pages are read only now and then, so the poll keeps them current: its copy of
 * a session replaces theirs, moving the row, and a session it carries (in
 * `polled`) that they don't have yet joins them. A remembered session isn't
 * polled, and may be archived, so only the pages list one. Until the first page
 * lands, the poll's recent ones matching the search stand in.
 */
export function visibleSessions(
  pages: ReturnType<typeof useSessionPages>,
  recent: SessionSummary[],
  query: string,
  edited: (session: SessionSummary) => SessionSummary | null,
  polled: ReadonlySet<string>,
) {
  const found = (session: SessionSummary) =>
    [session.title, session.model, session.branch].some((field) =>
      field.toLowerCase().includes(query),
    );
  const live = new Map(recent.map((session) => [session.id, session]));
  const read = new Set(pages?.sessions.map((session) => session.id));
  const listed = pages
    ? [
        ...pages.sessions.flatMap((session) => live.get(session.id) ?? edited(session) ?? []),
        ...recent.filter(
          (session) => !read.has(session.id) && polled.has(session.id) && found(session),
        ),
      ]
    : recent.filter(found);
  return listed.sort((a, b) => b.updatedAt - a.updatedAt);
}
