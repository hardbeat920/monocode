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
  /** The session of the Mono showing now, if any. */
  shownMonoSession: () => string | null;
  /** Bumped whenever a Mono opens or closes. */
  viewRequest: () => number;
  monoOfSession: (sessionId: string) => string | undefined;
  hasSession: (sessionId: string) => boolean;
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
  const restore = useRef<
    { sessionId: string; view: MonoOverviewView } | undefined
  >(undefined);
  // Documents deleted this session; a restore in flight checks them when it
  // applies, after it was handed over.
  const deleted = useRef(new Set<string>());

  // Recording a return renders once more: an open that landed in a commit of
  // its own (flushSync, into the tab already active) has no pass after it.
  const [recorded, setRecorded] = useState(0);

  // Focus moving off the file, to another file, pane or tab, ends the return.
  useEffect(() => {
    if (!back.current) return;
    const tab = host.tabs.find((entry) => entry.id === host.activeTabId);
    back.current = trackMonoReturn(
      back.current,
      host.activeTabId,
      tab && focusedFileTab(tab)?.path,
    );
  }, [host.tabs, host.activeTabId, recorded]);

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
            back.current = { monoId, sessionId, tabId, path, view };
            setRecorded((count) => count + 1);
          },
        };
      },
      /** The user closed these files in `tabId`. */
      fileClosed(tabId: string, paths: readonly string[]) {
        const found = monoReturnFor(back.current, { tabId, paths });
        // A file never seen in view was opened in the background.
        if (!found?.seen) return;
        back.current = undefined;
        // A Mono already showing, or one that is gone, leaves the close alone.
        if (
          hostRef.current.shownMonoSession() ||
          !hostRef.current.canOpenMono(found.monoId)
        ) {
          return;
        }
        restore.current = { sessionId: found.sessionId, view: found.view };
        hostRef.current.openMono(found.monoId);
      },
      /** A Mono is opening, whichever it is: ends any return. Once its session
       * is known, the returned function gives the view to put back, if the
       * open is the return's own, minus what no longer exists. */
      monoOpening() {
        back.current = undefined;
        const taken = restore.current;
        restore.current = undefined;
        return (sessionId: string) =>
          taken?.sessionId === sessionId
            ? restorableMonoView(taken.view, hostRef.current.hasSession, (id) =>
                deleted.current.has(id),
              )
            : null;
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
