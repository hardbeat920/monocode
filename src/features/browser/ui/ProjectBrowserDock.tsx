import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { DockPanel } from "../../workspace/ui/DockPanel";
import {
  applyDockGridStyle,
  findProjectTerminal,
} from "../../projects/model/projectTerminal";
import { sameProjectPath } from "../../projects/model/recents";
import {
  onBrowserEvent,
  retainBrowserViews,
} from "../../../platform/tauri/browser";
import {
  browserDefaultSize,
  browserTabIds,
  closeBrowserTab,
  closeOtherBrowserTabs,
  dockOfTab,
  getBrowserState,
  hideBrowser,
  openBrowserTab,
  patchBrowserTab,
  reorderBrowserTabs,
  selectBrowserTab,
  setBrowserProject,
  setBrowserSide,
  setBrowserSize,
  useBrowserState,
  type BrowserDock,
} from "../model/browserStore";
import { BrowserView } from "./BrowserView";

function ProjectBrowserDock({
  dock,
  visible,
  onSizePaint,
  onSizeCommit,
}: {
  dock: BrowserDock;
  visible: boolean;
  onSizePaint: (size: number) => void;
  onSizeCommit: (size: number) => void;
}) {
  const [resizing, setResizing] = useState(false);
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
      {dock.pane.files.map((file) => {
        const active = file.id === dock.pane.activeFileId;
        return file.browser ? (
          <div
            key={file.id}
            aria-hidden={!active}
            className={active ? "absolute inset-0 h-full" : "hidden"}
          >
            <BrowserView
              id={file.id}
              tab={file.browser}
              visible={visible && active}
              resizing={resizing}
            />
          </div>
        ) : null;
      })}
    </DockPanel>
  );
}

/** Keep this window's tabs in step with their native pages. */
function useBrowserEvents() {
  useEffect(() => {
    void retainBrowserViews(browserTabIds()).catch(() => undefined);
    const unlisten = onBrowserEvent((event) => {
      if (event.kind === "load") {
        patchBrowserTab(event.id, { url: event.url, loading: event.loading });
      } else if (event.kind === "title") {
        patchBrowserTab(event.id, { title: event.title });
      } else {
        const dock = dockOfTab(getBrowserState(), event.id);
        openBrowserTab(event.url, {
          projectPath: dock?.projectPath,
          afterId: event.id,
        });
      }
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, []);
}

/**
 * Lays the current project's browser dock beside `children`, the same way the
 * terminal dock sits beside the workspace. Other projects' docks stay mounted
 * but hidden so their pages keep running.
 */
export function BrowserDockLayout({
  projectPath,
  hidden,
  children,
}: {
  projectPath: string;
  /** Something covers the workspace (settings, inbox, a full-view Mono). */
  hidden: boolean;
  children: ReactNode;
}) {
  const state = useBrowserState();
  const grid = useRef<HTMLDivElement>(null);
  const dragging = useRef(false);
  useBrowserEvents();

  useLayoutEffect(() => {
    setBrowserProject(projectPath);
  }, [projectPath]);

  const current = findProjectTerminal(state.docks, state.projectPath);
  const showSide = current?.open && !hidden ? current.side : null;
  const size = current?.size ?? 0;

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
      {state.docks.map((dock) => {
        const show =
          dock.open &&
          !hidden &&
          sameProjectPath(dock.projectPath, state.projectPath);
        return (
          <div
            key={dock.projectPath}
            className={
              show ? "h-full min-h-0 min-w-0 w-full overflow-hidden" : "hidden"
            }
            style={show ? { gridArea: "dock" } : undefined}
            aria-hidden={!show}
          >
            <ProjectBrowserDock
              dock={dock}
              visible={show}
              onSizePaint={paint}
              onSizeCommit={commit}
            />
          </div>
        );
      })}
      <div
        className="relative flex min-h-0 min-w-0"
        style={{ gridArea: "main" }}
      >
        {children}
      </div>
    </div>
  );
}
