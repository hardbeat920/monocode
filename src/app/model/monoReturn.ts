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
  /** The render that recorded this: the first pass of it or a later one sees
   * the committed state, an earlier one is stale. */
  generation: number;
  /** The file has been seen focused in its tab; losing that ends the return,
   * and a close only returns once it has. */
  seen?: boolean;
};

/** Keeps a return while its file stays the focused one in its tab, with
 * nothing over it. Any other focus, file, tab or view ends it, so a later
 * close is just a close. A file opened behind the active tab is never in
 * view, and ends here. Until the file has landed in its tab (a transition
 * may commit it after the open recorded), there is nothing to judge yet. */
export function trackMonoReturn(
  back: MonoReturn | undefined,
  activeTabId: string,
  focusedPath: string | undefined,
  covered: boolean,
  landed: boolean,
): MonoReturn | undefined {
  if (!back) return undefined;
  if (!landed) return back.seen ? undefined : back;
  const inView =
    !covered && back.tabId === activeTabId && back.path === focusedPath;
  if (!inView) return undefined;
  return back.seen ? back : { ...back, seen: true };
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
      document:
        document && view.scroll.document?.id === document
          ? view.scroll.document
          : undefined,
    },
    search: view.search,
    activity:
      view.activity && hasSession(view.activity.sessionId)
        ? view.activity
        : null,
  };
}
