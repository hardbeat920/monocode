import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { DockPanel } from "../../workspace/ui/DockPanel";
import { applyDockGridStyle } from "../../projects/model/projectTerminal";
import {
  onBrowserEvent,
  readBrowserUrl,
  retainBrowserViews,
} from "../../../platform/tauri/browser";
import {
  browserDefaultSize,
  browserTabIds,
  closeBrowserTab,
  closeOtherBrowserTabs,
  dockOfTab,
  findBrowserDock,
  getBrowserState,
  hideBrowser,
  liveBrowserTabIds,
  openBrowserTab,
  patchBrowserTab,
  reorderBrowserTabs,
  selectBrowserTab,
  setBrowserSession,
  setBrowserSide,
  setBrowserSize,
  useBrowserState,
  type BrowserDock,
  noteLoadFinished,
  noteLoadStarted,
} from "../model/browserStore";
import { syncNativeUrl, trackedBrowserTabIds } from "../model/nativeUrl";
import { BrowserSurface, BrowserToolbar } from "./BrowserView";

/** How often live pages are asked for a URL that changed without a load. */
const URL_POLL_MS = 1_000;

/** Size of the off-window page an agent works in while the panel is hidden. */
const BACKGROUND_SIZE = { width: 1280, height: 800 };

function SessionBrowserDock({
  dock,
  onSizePaint,
  onSizeCommit,
}: {
  dock: BrowserDock;
  onSizePaint: (size: number) => void;
  onSizeCommit: (size: number) => void;
}) {
  const [resizing, setResizing] = useState(false);
  const active = dock.pane.files.find(
    (file) => file.id === dock.pane.activeFileId,
  );
  return (
    <DockPanel
      dock={dock}
      name="Browser"
      tabsLabel="Browser tabs"
      addLabel="New Browser Tab"
      hideLabel="Hide Browser"
      defaultSize={browserDefaultSize}
      sashOutside
      onHide={hideBrowser}
      onSideChange={setBrowserSide}
      onSizePaint={onSizePaint}
      onSizeCommit={onSizeCommit}
      onResizingChange={setResizing}
      onAdd={() => openBrowserTab()}
      onSelect={selectBrowserTab}
      onClose={closeBrowserTab}
      onCloseOthers={closeOtherBrowserTabs}
      onReorder={reorderBrowserTabs}
    >
      {active?.browser ? (
        // One key on the wrapper: switching tabs swaps toolbar and page together.
        <div
          key={active.id}
          className="absolute inset-0 flex h-full min-h-0 flex-col"
        >
          <BrowserToolbar id={active.id} tab={active.browser} />
          <BrowserSurface
            id={active.id}
            tab={active.browser}
            visible
            resizing={resizing}
          />
        </div>
      ) : null}
    </DockPanel>
  );
}

/** Keep this window's tabs in step with their native pages. */
function useBrowserEvents() {
  useEffect(() => {
    void retainBrowserViews(browserTabIds(getBrowserState())).catch(
      () => undefined,
    );
    const unlisten = onBrowserEvent((event) => {
      if (event.kind === "load") {
        if (event.loading) noteLoadStarted(event.id);
        else noteLoadFinished(event.id);
        patchBrowserTab(event.id, {
          url: event.url,
          loading: event.loading,
          ...(event.loading ? { error: undefined } : {}),
        });
      } else if (event.kind === "title") {
        patchBrowserTab(event.id, { title: event.title });
      } else {
        const dock = dockOfTab(getBrowserState(), event.id);
        if (!dock) return;
        openBrowserTab(event.url, {
          sessionId: dock.sessionId,
          afterId: event.id,
          background: !dock.open,
        });
      }
    });
    // pushState, replaceState and hash changes emit no native load event.
    let polling = false;
    const poll = setInterval(() => {
      if (polling) return;
      polling = true;
      void Promise.all(
        trackedBrowserTabIds().map((id) => syncNativeUrl(id, readBrowserUrl)),
      ).finally(() => {
        polling = false;
      });
    }, URL_POLL_MS);
    return () => {
      clearInterval(poll);
      void unlisten.then((fn) => fn());
    };
  }, []);
}

/**
 * Lays the focused session's browser dock beside `children`, the way the
 * terminal dock sits beside the workspace. Only the docked tab draws on
 * screen. Other live tabs (the session's other tabs, recently viewed
 * sessions' tabs, and tabs an agent is using) keep running in an off-window
 * host, so switching sessions never reloads them. Past the live-page limit
 * the least recently viewed are suspended to their URL.
 */
export function BrowserDockLayout({
  sessionId,
  hidden,
  children,
}: {
  /** The focused session, or "" when none. */
  sessionId: string;
  /** Something covers the workspace (settings, inbox, search). */
  hidden: boolean;
  children: ReactNode;
}) {
  const state = useBrowserState();
  const grid = useRef<HTMLDivElement>(null);
  const dragging = useRef(false);
  useBrowserEvents();

  useLayoutEffect(() => {
    setBrowserSession(sessionId);
  }, [sessionId]);

  const current = findBrowserDock(state.docks, state.sessionId);
  const show = !!current?.open && !hidden;
  const showSide = show ? current.side : null;
  const size = current?.size ?? 0;
  const dockedId = show ? current.pane.activeFileId : null;
  const live = liveBrowserTabIds(state);
  const background = state.docks.flatMap((dock) =>
    dock.pane.files.filter(
      (file) => file.browser && live.has(file.id) && file.id !== dockedId,
    ),
  );

  useLayoutEffect(() => {
    if (dragging.current || !grid.current) return;
    applyDockGridStyle(grid.current, showSide, size);
  }, [showSide, size]);

  const paint = useCallback(
    (next: number) => {
      if (!grid.current || !showSide) return;
      dragging.current = true;
      applyDockGridStyle(grid.current, showSide, next);
    },
    [showSide],
  );
  const commit = useCallback((next: number) => {
    dragging.current = false;
    setBrowserSize(next);
  }, []);

  return (
    <div ref={grid} className="grid h-full min-h-0 min-w-0 flex-1">
      {show ? (
        <div
          key={current.sessionId}
          className="h-full min-h-0 min-w-0 w-full"
          style={{ gridArea: "dock" }}
        >
          <SessionBrowserDock
            dock={current}
            onSizePaint={paint}
            onSizeCommit={commit}
          />
        </div>
      ) : null}
      <div
        className="relative flex min-h-0 min-w-0"
        style={{ gridArea: "main" }}
      >
        {children}
      </div>
      {background.length > 0 ? (
        <div
          aria-hidden
          inert
          className="pointer-events-none fixed top-0 flex"
          style={{
            left: -(BACKGROUND_SIZE.width + 10_000),
            width: BACKGROUND_SIZE.width,
            height: BACKGROUND_SIZE.height,
          }}
        >
          {background.map((file) =>
            file.browser ? (
              <BrowserSurface
                key={file.id}
                id={file.id}
                tab={file.browser}
                visible
                className="absolute inset-0"
              />
            ) : null,
          )}
        </div>
      ) : null}
    </div>
  );
}
