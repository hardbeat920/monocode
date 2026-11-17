import type { MonoActivitySelection } from "../../features/monos/model/monoActivity";
import type { OverviewSearch } from "../../features/monos/ui/MonoOverview";
import type { MonoScroll } from "../../features/monos/model/monoScroll";

/** What a Mono's overview showed over its chat. */
export type MonoOverviewView = {
  shownSession: string | null;
  shownSessionFocused: boolean;
  document: string | null;
  activity: MonoActivitySelection | null;
  scroll?: MonoScroll;
  search?: OverviewSearch;
};

/** The Mono a file was opened from, so closing that file returns to it. */
export type MonoReturn = {
  monoId: string;
  sessionId: string;
  tabId: string;
  path: string;
  view: MonoOverviewView;
  /** The file has been seen focused in its tab; losing that ends the return,
   * and a close only returns once it has. */
  seen?: boolean;
  /** Passes that found it not focused, before it was ever seen. */
  misses?: number;
};

/** Passes allowed to miss a fresh open: effects of a commit older than the
 * open may run after it, once. The next pass sees the file or it never will
 * (opened in the background, or the open failed). */
export const MAX_UNSEEN_PASSES = 2;

/** Keeps a return while its file stays the focused one in its tab. Any other
 * focus, file or tab ends it, so a later close is just a close. A return not
 * yet seen focused is kept for a pass: the open may not have rendered. */
export function trackMonoReturn(
  back: MonoReturn | undefined,
  activeTabId: string,
  focusedPath: string | undefined,
): MonoReturn | undefined {
  if (!back) return undefined;
  const inView = back.tabId === activeTabId && back.path === focusedPath;
  if (inView) return back.seen ? back : { ...back, seen: true };
  if (back.seen) return undefined;
  const misses = (back.misses ?? 0) + 1;
  return misses >= MAX_UNSEEN_PASSES ? undefined : { ...back, misses };
}

/** The return a close fulfils: that file closed in its own tab. */
export function monoReturnFor(
  back: MonoReturn | undefined,
  closed: { tabId: string; paths: readonly string[] },
): MonoReturn | undefined {
  return back?.tabId === closed.tabId && closed.paths.includes(back.path)
    ? back
    : undefined;
}

/** The view minus whatever no longer exists. */
export function restorableMonoView(
  view: MonoOverviewView,
  hasSession: (id: string) => boolean,
  isDeleted: (documentId: string) => boolean,
): MonoOverviewView {
  const shownSession =
    view.shownSession && hasSession(view.shownSession)
      ? view.shownSession
      : null;
  const document =
    view.document && !isDeleted(view.document) ? view.document : null;
  return {
    shownSession,
    shownSessionFocused: !!shownSession && view.shownSessionFocused,
    document,
    // A document that is gone takes its scroll with it.
    scroll: view.scroll && {
      ...view.scroll,
      document: document ? view.scroll.document : undefined,
    },
    search: view.search,
    activity:
      view.activity && hasSession(view.activity.sessionId)
        ? view.activity
        : null,
  };
}
