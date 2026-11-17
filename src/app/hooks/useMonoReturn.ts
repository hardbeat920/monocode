import { useEffect, useMemo, useRef, useState } from "react";
import {
  focusedFileTab,
  type WorkspaceTab,
} from "../../features/workspace/model/layout";
import {
  monoReturnFor,
  restorableMonoView,
  trackMonoReturn,
  type MonoOverviewView,
  type MonoReturn,
} from "../model/monoReturn";

type Host = {
  tabs: readonly WorkspaceTab[];
  activeTabId: string;
  /** Something other than the workspace shows: a Mono, an app view, settings. */
  covered: boolean;
  /** The session of the Mono showing now, if any. */
  shownMonoSession: () => string | null;
  /** Bumped whenever a Mono opens or closes. */
  viewRequest: () => number;
  monoOfSession: (sessionId: string) => string | undefined;
  /** Whether the session still exists, loading it when it was parked. */
  hasSession: (sessionId: string) => Promise<boolean>;
  /** Whether the Mono exists and Monos are on. */
  canOpenMono: (monoId: string) => boolean;
  openMono: (monoId: string) => void;
  view: MonoOverviewView;
  /** What the shown overview holds that its Mono's state does not: where it
   * is scrolled and its search. Read before it unmounts. */
  readOverview: () => Pick<MonoOverviewView, "scroll" | "search">;
};

/** Closing a file opened from a Mono returns to that Mono, and its overview
 * as it was. Only closing that file does; any other navigation ends it. */
export function useMonoReturn(host: Host) {
  const hostRef = useRef(host);
  hostRef.current = host;
  const back = useRef<MonoReturn | undefined>(undefined);
  // The return of a close under way. Closing moves on to a neighbouring tab,
  // which may be this very Mono's: that open takes the return over.
  const closing = useRef<MonoReturn | undefined>(undefined);
  // Documents deleted this session; a restore in flight checks them when it
  // applies, after it was handed over.
  const deleted = useRef(new Set<string>());
  const generation = useRef(0);

  // Recording a return renders once more, whose pass sees what the open left:
  // an open that landed in a commit of its own (flushSync, into the tab
  // already active) has no other pass after it.
  const [recorded, setRecorded] = useState(0);

  // Focus moving off the file, to another file, pane, tab or view, ends the
  // return. A pass older than the record is stale and says nothing.
  useEffect(() => {
    if (!back.current || recorded < back.current.generation) return;
    const tab = host.tabs.find((entry) => entry.id === host.activeTabId);
    const { tabId, path } = back.current;
    const landed = !!host.tabs
      .find((entry) => entry.id === tabId)
      ?.editorPanes.some((pane) => pane.files.some((f) => f.path === path));
    back.current = trackMonoReturn(
      back.current,
      host.activeTabId,
      tab && focusedFileTab(tab)?.path,
      host.covered,
      landed,
    );
  }, [host.tabs, host.activeTabId, host.covered, recorded]);

  // Every method reads refs only, so the object stays the same.
  return useMemo(
    () => ({
      /** Call as a file opens: `close` leaves the Mono showing now. Any earlier
       * return ends, so reopening a file starts fresh. */
      beginFileOpen(close: () => void) {
        back.current = undefined;
        const sessionId = hostRef.current.shownMonoSession();
        const monoId = sessionId
          ? hostRef.current.monoOfSession(sessionId)
          : undefined;
        // Read before closing: the overview unmounts with the Mono.
        const view = {
          ...hostRef.current.view,
          ...hostRef.current.readOverview(),
        };
        close();
        const request = hostRef.current.viewRequest();
        let untouched = false;
        return {
          /** Once the file resolved, before this open moves anything itself. */
          resolved() {
            untouched = request === hostRef.current.viewRequest();
          },
          /** The file opened in `tabId`. Not kept when the user went to or
           * from a Mono while it resolved. */
          opened(tabId: string, path: string) {
            if (!sessionId || !monoId || !untouched) return;
            generation.current += 1;
            back.current = {
              monoId,
              sessionId,
              tabId,
              path,
              view,
              generation: generation.current,
            };
            setRecorded(generation.current);
          },
        };
      },
      /** The user is closing these files in `tabId`, confirmed, and the close
       * is about to move on. Pair with `closeFinished`. */
      closeStarting(tabId: string, paths: readonly string[]) {
        const found = monoReturnFor(back.current, { tabId, paths });
        // A file never seen in view was opened in the background.
        if (!found?.seen) return;
        back.current = undefined;
        closing.current = found;
      },
      /** The close has moved on: return to the Mono unless a Mono opened
       * meanwhile and took the return over. */
      closeFinished() {
        const found = closing.current;
        if (!found) return;
        // A Mono already showing, or one that is gone, leaves the close alone.
        if (
          !hostRef.current.shownMonoSession() &&
          hostRef.current.canOpenMono(found.monoId)
        ) {
          hostRef.current.openMono(found.monoId); // takes `closing`
        }
        closing.current = undefined;
      },
      /** A Mono is opening, whichever it is: ends any return, except that of
       * a close under way for this same Mono, which is what it returns to.
       * Once its session is known, the returned function gives the view to put
       * back, minus what no longer exists. */
      monoOpening(monoId: string) {
        back.current = undefined;
        const taken =
          closing.current?.monoId === monoId ? closing.current : undefined;
        if (taken) closing.current = undefined;
        return async (sessionId: string) => {
          if (taken?.sessionId !== sessionId) return null;
          const { shownSession, activity } = taken.view;
          const live = new Set<string>();
          for (const id of [shownSession, activity?.sessionId]) {
            if (id && (await hostRef.current.hasSession(id))) live.add(id);
          }
          // After the awaits, so a deletion during them is seen.
          return restorableMonoView(
            taken.view,
            (id) => live.has(id),
            (id) => deleted.current.has(id),
          );
        };
      },
      /** A document is gone, so no view brings it back, whether it is
       * waiting here or already handed to a Mono opening. */
      documentDeleted(id: string) {
        deleted.current.add(id);
      },
    }),
    [],
  );
}
