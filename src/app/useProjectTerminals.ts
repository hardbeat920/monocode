import {
  useCallback,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type Dispatch,
  type MutableRefObject,
  type SetStateAction,
} from "react";
import {
  newDefaultSession,
  sessionWorkCwd,
  type Session,
} from "../lib/session";
import {
  focusedFileTab,
  newTerminalFile,
  newTerminalWorkspaceTab,
  nextTerminalTitle,
  openTerminalTab,
  splitPane,
  updateTerminalTab,
  withSurfacePanes,
  type FilePaneTab,
  type SplitDir,
  type WorkspaceTab,
} from "../lib/layout";
import { looksLikeProject } from "../lib/recents";
import {
  addTerminalToDock,
  closeTerminalInDock,
  createProjectTerminal,
  findProjectTerminal,
  mapProjectTerminal,
  nextDockTerminalTitle,
  patchProjectTerminals,
  reorderDockTerminals,
  selectDockTerminal,
  withDockOpen,
  withDockSide,
  withDockSize,
  type DockSide,
  type ProjectTerminalDock as ProjectTerminal,
} from "../lib/projectTerminal";
import { isBlankSession } from "../lib/projectReturn";
import { forgetHarnessSession } from "../lib/harness";
import { orderByIds } from "../lib/reorder";
import {
  confirmCloseTerminal,
  confirmCloseTerminals,
} from "../lib/terminalClose";
import {
  listRunningTerminals,
  type TerminalMetaPatch,
} from "../lib/terminalTab";

export interface ProjectTerminalsDeps {
  initialTerminals: ProjectTerminal[];
  projectCwd: string;
  projectCwdRef: MutableRefObject<string>;
  activeTab: WorkspaceTab | undefined;
  active: Session | undefined;
  sessionDefaults: Session | undefined;
  sessionsRef: MutableRefObject<Session[]>;
  tabsRef: MutableRefObject<WorkspaceTab[]>;
  activeTabIdRef: MutableRefObject<string>;
  tabs: WorkspaceTab[];
  appendTab: (tab: WorkspaceTab, cwd?: string) => void;
  lastPersisted: MutableRefObject<Map<string, string>>;
  setSessions: Dispatch<SetStateAction<Session[]>>;
  setTabs: Dispatch<SetStateAction<WorkspaceTab[]>>;
  setActiveTabId: Dispatch<SetStateAction<string>>;
  setComposerFocused: Dispatch<SetStateAction<boolean>>;
}

export function useProjectTerminals(deps: ProjectTerminalsDeps) {
  const {
    initialTerminals,
    projectCwd,
    projectCwdRef,
    activeTab,
    active,
    sessionDefaults,
    sessionsRef,
    tabsRef,
    activeTabIdRef,
    tabs,
    appendTab,
    lastPersisted,
    setSessions,
    setTabs,
    setActiveTabId,
    setComposerFocused,
  } = deps;
  const [projectTerminals, setProjectTerminals] = useState<ProjectTerminal[]>(
    () => initialTerminals,
  );
  const [projectTerminalFocused, setProjectTerminalFocused] = useState(false);
  const projectTerminalsRef = useRef(projectTerminals);
  const projectTerminalFocusedRef = useRef(projectTerminalFocused);
  useLayoutEffect(() => {
    projectTerminalsRef.current = projectTerminals;
    projectTerminalFocusedRef.current = projectTerminalFocused;
  }, [projectTerminals, projectTerminalFocused]);
  const onSplit = useCallback(
    (dir: SplitDir) => {
      if (!activeTab) return;
      const session = newDefaultSession(
        sessionDefaults?.cwd ?? projectCwd,
        sessionDefaults?.runtimeMode,
      );
      setSessions((prev) => [...prev, session]);
      setTabs((prev) =>
        prev.map((t) => {
          if (t.id !== activeTab.id) return t;
          return {
            ...t,
            layout: splitPane(t.layout, t.focusedId, dir, session.id),
            focusedId: session.id,
          };
        }),
      );
      setComposerFocused(true);
    },
    [activeTab, projectCwd, sessionDefaults?.cwd, sessionDefaults?.runtimeMode],
  );

  const focusProjectTerminal = useCallback(() => {
    setProjectTerminalFocused(true);
    setComposerFocused(false);
  }, []);

  const openProjectTerminal = useCallback(
    (cwd: string) => {
      const workdir = cwd || projectCwdRef.current;
      const projectPath = projectCwdRef.current;
      if (!looksLikeProject(projectPath)) return false;
      setProjectTerminals((prev) => {
        const existing = findProjectTerminal(prev, projectPath);
        const file = newTerminalFile(
          workdir,
          existing ? nextDockTerminalTitle(existing, workdir) : undefined,
        );
        if (!existing) {
          return [...prev, createProjectTerminal(projectPath, file)];
        }
        return mapProjectTerminal(prev, projectPath, (dock) =>
          addTerminalToDock(dock, file),
        );
      });
      focusProjectTerminal();
      return true;
    },
    [focusProjectTerminal],
  );

  const onOpenTerminal = useCallback(
    (cwd: string, asWorkspaceTab = false, occupySessionId?: string) => {
      const workdir = cwd || active?.cwd || projectCwd;
      if (openProjectTerminal(workdir)) return;

      if (asWorkspaceTab || !activeTab) {
        const file = newTerminalFile(workdir);
        const tab = newTerminalWorkspaceTab(file);
        appendTab(tab, workdir);
        setActiveTabId(tab.id);
        setComposerFocused(false);
        return;
      }

      const occupying = sessionsRef.current.find(
        (session) => session.id === (occupySessionId ?? activeTab.focusedId),
      );
      const occupyPaneId =
        occupying && isBlankSession(occupying) ? occupying.id : undefined;
      if (occupyPaneId && occupying) {
        lastPersisted.current.delete(occupyPaneId);
        void forgetHarnessSession(occupying.harness, occupyPaneId);
        setSessions((prev) =>
          prev.filter((session) => session.id !== occupyPaneId),
        );
      }

      const file = newTerminalFile(
        workdir,
        nextTerminalTitle(activeTab, workdir),
      );
      setTabs((prev) =>
        prev.map((tab) =>
          tab.id === activeTab.id
            ? openTerminalTab(tab, file, occupyPaneId)
            : tab,
        ),
      );
      setComposerFocused(false);
    },
    [active?.cwd, activeTab, appendTab, openProjectTerminal, projectCwd],
  );

  const onNewTerminal = useCallback(() => {
    onOpenTerminal(active?.cwd ?? projectCwd);
  }, [active?.cwd, onOpenTerminal, projectCwd]);

  const onShowProjectTerminal = useCallback(() => {
    const dock = findProjectTerminal(projectTerminalsRef.current, projectCwd);
    if (dock && dock.pane.files.length > 0) {
      if (!dock.open) {
        setProjectTerminals((prev) =>
          mapProjectTerminal(prev, projectCwd, (entry) =>
            withDockOpen(entry, true),
          ),
        );
      }
      focusProjectTerminal();
      return;
    }
    onOpenTerminal(active?.cwd ?? projectCwd);
  }, [active?.cwd, focusProjectTerminal, onOpenTerminal, projectCwd]);

  const onNewTerminalInSession = useCallback(
    (sessionId: string) => {
      const session = sessionsRef.current.find(
        (entry) => entry.id === sessionId,
      );
      onOpenTerminal(
        session ? sessionWorkCwd(session) : projectCwd,
        false,
        sessionId,
      );
    },
    [onOpenTerminal, projectCwd],
  );

  const onToggleProjectTerminal = useCallback(() => {
    if (!looksLikeProject(projectCwd)) return;
    const dock = findProjectTerminal(projectTerminalsRef.current, projectCwd);
    if (!dock) {
      openProjectTerminal(active?.cwd ?? projectCwd);
      return;
    }
    const nextOpen = !dock.open;
    setProjectTerminals((prev) =>
      mapProjectTerminal(prev, projectCwd, (entry) =>
        withDockOpen(entry, nextOpen),
      ),
    );
    if (nextOpen) focusProjectTerminal();
    else setProjectTerminalFocused(false);
  }, [active?.cwd, focusProjectTerminal, openProjectTerminal, projectCwd]);

  const onHideProjectTerminal = useCallback(() => {
    setProjectTerminals((prev) =>
      mapProjectTerminal(prev, projectCwdRef.current, (dock) =>
        withDockOpen(dock, false),
      ),
    );
    setProjectTerminalFocused(false);
  }, []);

  const onProjectTerminalSide = useCallback((side: DockSide) => {
    setProjectTerminals((prev) =>
      mapProjectTerminal(prev, projectCwdRef.current, (dock) =>
        withDockSide(dock, side, {
          width: window.innerWidth,
          height: window.innerHeight,
        }),
      ),
    );
  }, []);

  const onProjectTerminalSize = useCallback((size: number) => {
    setProjectTerminals((prev) =>
      mapProjectTerminal(prev, projectCwdRef.current, (dock) =>
        withDockSize(dock, size, {
          width: window.innerWidth,
          height: window.innerHeight,
        }),
      ),
    );
  }, []);

  const onSelectProjectTerminal = useCallback(
    (fileId: string) => {
      setProjectTerminals((prev) =>
        mapProjectTerminal(prev, projectCwdRef.current, (dock) =>
          selectDockTerminal(dock, fileId),
        ),
      );
      focusProjectTerminal();
    },
    [focusProjectTerminal],
  );

  const onReorderProjectTerminals = useCallback((ids: string[]) => {
    setProjectTerminals((prev) =>
      mapProjectTerminal(prev, projectCwdRef.current, (dock) =>
        reorderDockTerminals(dock, orderByIds(dock.pane.files, ids)),
      ),
    );
  }, []);

  const onCloseProjectTerminal = useCallback((fileId: string) => {
    const dock = findProjectTerminal(
      projectTerminalsRef.current,
      projectCwdRef.current,
    );
    const file = dock?.pane.files.find((entry) => entry.id === fileId);
    if (!file) return;
    const finishClose = () => {
      setProjectTerminals((prev) =>
        mapProjectTerminal(prev, projectCwdRef.current, (entry) =>
          closeTerminalInDock(entry, fileId),
        ),
      );
    };
    void confirmCloseTerminal(file).then((ok) => ok && finishClose());
  }, []);

  const onCloseOtherProjectTerminals = useCallback((fileId: string) => {
    const projectPath = projectCwdRef.current;
    const dock = findProjectTerminal(projectTerminalsRef.current, projectPath);
    if (!dock?.pane.files.some((file) => file.id === fileId)) return;
    const closingFiles = dock.pane.files.filter((file) => file.id !== fileId);
    if (closingFiles.length === 0) return;
    const closingIds = new Set(closingFiles.map((file) => file.id));

    const finishClose = () => {
      setProjectTerminals((prev) =>
        mapProjectTerminal(prev, projectPath, (entry) => {
          if (!entry.pane.files.some((file) => file.id === fileId)) {
            return entry;
          }
          const files = entry.pane.files.filter(
            (file) => !closingIds.has(file.id),
          );
          return {
            ...entry,
            pane: { ...entry.pane, files, activeFileId: fileId },
          };
        }),
      );
    };

    void confirmCloseTerminals(closingFiles).then((ok) => ok && finishClose());
  }, []);

  const onTerminalMetaChange = useCallback(
    (fileId: string, patch: TerminalMetaPatch) => {
      setProjectTerminals((prev) => patchProjectTerminals(prev, fileId, patch));
      setTabs((prev) =>
        prev.map((tab) => updateTerminalTab(tab, fileId, patch)),
      );
    },
    [],
  );

  const onToggleRunningTerminal = useCallback(
    (fileId: string) => {
      const dock = projectTerminalsRef.current.find((entry) =>
        entry.pane.files.some((file) => file.id === fileId),
      );
      if (dock) {
        if (dock.open) {
          setProjectTerminals((prev) =>
            mapProjectTerminal(prev, dock.projectPath, (entry) =>
              withDockOpen(entry, false),
            ),
          );
          setProjectTerminalFocused(false);
          return;
        }
        setProjectTerminals((prev) =>
          mapProjectTerminal(prev, dock.projectPath, (entry) =>
            withDockOpen(selectDockTerminal(entry, fileId), true),
          ),
        );
        focusProjectTerminal();
        return;
      }
      for (const tab of tabsRef.current) {
        for (const pane of tab.terminalPanes ?? []) {
          if (!pane.files.some((file) => file.id === fileId)) continue;
          const showing =
            activeTabIdRef.current === tab.id &&
            tab.focusedId === pane.id &&
            pane.activeFileId === fileId;
          if (showing) {
            setComposerFocused(true);
            setProjectTerminalFocused(false);
            return;
          }
          setActiveTabId(tab.id);
          setTabs((prev) =>
            prev.map((entry) => {
              if (entry.id !== tab.id) return entry;
              return withSurfacePanes(
                { ...entry, focusedId: pane.id },
                "terminal",
                (entry.terminalPanes ?? []).map((item) =>
                  item.id === pane.id
                    ? { ...item, activeFileId: fileId }
                    : item,
                ),
              );
            }),
          );
          setProjectTerminalFocused(false);
          setComposerFocused(false);
          return;
        }
      }
    },
    [focusProjectTerminal],
  );

  const onNewTerminalTab = useCallback(() => {
    onOpenTerminal(active?.cwd ?? projectCwd, true);
  }, [active?.cwd, onOpenTerminal, projectCwd]);
  const currentProjectDock = findProjectTerminal(projectTerminals, projectCwd);
  const dockVisible = !!currentProjectDock?.open;
  const runningTerminals = useMemo(() => {
    const files: FilePaneTab[] = [];
    const dock = findProjectTerminal(projectTerminals, projectCwd);
    if (dock) files.push(...dock.pane.files);
    for (const tab of tabs) {
      for (const pane of tab.terminalPanes ?? []) {
        files.push(...pane.files);
      }
    }
    return listRunningTerminals(files);
  }, [projectCwd, projectTerminals, tabs]);
  const runningTerminalOpen = useMemo(() => {
    const ids = new Set(runningTerminals.map((terminal) => terminal.id));
    if (
      currentProjectDock?.open &&
      currentProjectDock.pane.files.some((file) => ids.has(file.id))
    ) {
      return true;
    }
    const focused = activeTab ? focusedFileTab(activeTab) : undefined;
    return !!focused && ids.has(focused.id);
  }, [activeTab, currentProjectDock, runningTerminals]);

  return {
    projectTerminals,
    setProjectTerminals,
    projectTerminalFocused,
    setProjectTerminalFocused,
    projectTerminalsRef,
    projectTerminalFocusedRef,
    currentProjectDock,
    dockVisible,
    runningTerminals,
    runningTerminalOpen,
    onSplit,
    focusProjectTerminal,
    openProjectTerminal,
    onOpenTerminal,
    onNewTerminal,
    onShowProjectTerminal,
    onNewTerminalInSession,
    onToggleProjectTerminal,
    onHideProjectTerminal,
    onProjectTerminalSide,
    onProjectTerminalSize,
    onSelectProjectTerminal,
    onReorderProjectTerminals,
    onCloseProjectTerminal,
    onCloseOtherProjectTerminals,
    onTerminalMetaChange,
    onToggleRunningTerminal,
    onNewTerminalTab,
  };
}
