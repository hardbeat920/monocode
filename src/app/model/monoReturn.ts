import type { MonoActivitySelection } from "../../features/monos/model/monoActivity";

/** What a Mono's overview showed over its chat. */
export type MonoOverviewView = {
  shownSession: string | null;
  shownSessionFocused: boolean;
  document: string | null;
  activity: MonoActivitySelection | null;
};

/** The Mono a file was opened from, so closing that file returns to it. */
export type MonoReturn = {
  monoId: string;
  sessionId: string;
  tabId: string;
  path: string;
  view: MonoOverviewView;
  /** The file has been seen focused in its tab; losing that ends the return. */
  seen?: boolean;
};

/** Keeps a return while its file stays the focused one in its tab. Any other
 * focus, file or tab ends it, so a later close is just a close. A return not
 * yet seen focused is kept: the open may not have rendered. */
export function trackMonoReturn(
  back: MonoReturn | undefined,
  activeTabId: string,
  focusedPath: string | undefined,
): MonoReturn | undefined {
  if (!back) return undefined;
  const inView = back.tabId === activeTabId && back.path === focusedPath;
  if (inView) return back.seen ? back : { ...back, seen: true };
  return back.seen ? undefined : back;
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
): MonoOverviewView {
  const shownSession =
    view.shownSession && hasSession(view.shownSession)
      ? view.shownSession
      : null;
  return {
    shownSession,
    shownSessionFocused: !!shownSession && view.shownSessionFocused,
    document: view.document,
    activity:
      view.activity && hasSession(view.activity.sessionId)
        ? view.activity
        : null,
  };
}
