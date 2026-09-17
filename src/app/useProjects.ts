import {
  useCallback,
  type Dispatch,
  type MutableRefObject,
  type SetStateAction,
} from "react";
import {
  findSurfacePane,
  isFilesystemTab,
  leafIds,
  neighborLeafId,
  newFileTab,
  newPlanTab,
  newTab,
  openEditorTab,
  setSplitRatio,
  surfacePanes,
  withSurfacePanes,
  type FocusDir,
  type WorkspaceTab,
} from "../lib/layout";
import {
  archiveProject,
  forgetProject,
  looksLikeProject,
  normalizeProjectPath,
  rememberProject,
  sameProjectPath,
  type RecentProject,
} from "../lib/recents";
import {
  loadSessionFolders,
  placeSessionInFolder,
  saveSessionFolders,
  type SessionFolderTarget,
} from "../lib/sessionFolders";
import {
  invalidateProjectFiles,
  rememberOpenedFile,
  resolveFileOpenRequest,
} from "../lib/fileIndex";
import { rememberLoadedSession } from "../lib/sessionCache";
import {
  isEqualOrInside,
  projectName,
  rebasePath,
} from "../lib/paths";
import { notifyGitChanged, pickFolder } from "../lib/fs";
import type { OpenFileFn } from "../lib/search";
import type { EditorNavigationTarget } from "../lib/search";
import {
  cancelHarnessTurn,
  forgetHarnessSession,
} from "../lib/harness";
import { newDefaultSession, newSession, type Session } from "../lib/session";
import { isBlankSession, planProjectReturn } from "../lib/projectReturn";
import {
  keepSessionChanges,
  notifyReviewChanged,
} from "../lib/checkpoint";
import { planTitle } from "../lib/plan";
import { removeProjectData } from "../lib/projectData";
import { removeTabFromGroup, tabGroupProject } from "../lib/tabGroups";
import { sessionChildHarnesses } from "../lib/handoff";
import { shouldPersistSession } from "../lib/sessionStore";
import { dropOpenFiles } from "./tabHelpers";
import { filterTabsForProject } from "../lib/workspaceTabGroups";

export interface ProjectsDeps {
  activeTab: WorkspaceTab | undefined;
  projectCwd: string;
  sidebarCwd: string;
  sessions: Session[];
  tabs: import("../lib/layout").WorkspaceTab[];
  sessionsRef: MutableRefObject<Session[]>;
  tabsRef: MutableRefObject<WorkspaceTab[]>;
  activeTabIdRef: MutableRefObject<string>;
  projectCwdRef: MutableRefObject<string>;
  sidebarCwdRef: MutableRefObject<string>;
  gitCwdRef: MutableRefObject<string>;
  editorNavigationToken: MutableRefObject<number>;
  turnGen: MutableRefObject<Map<string, number>>;
  sessionLoads: MutableRefObject<Map<string, Promise<Session | null>>>;
  pendingPersist: MutableRefObject<Map<string, Session>>;
  loadedSessionCache: MutableRefObject<Map<string, Session>>;
  lastPersisted: MutableRefObject<Map<string, string>>;
  setSessions: Dispatch<SetStateAction<Session[]>>;
  setTabs: Dispatch<SetStateAction<WorkspaceTab[]>>;
  setDirtyFiles: Dispatch<SetStateAction<Set<string>>>;
  setFileErrorCounts: Dispatch<SetStateAction<Map<string, number>>>;
  setSearchViewOpen: Dispatch<SetStateAction<boolean>>;
  setInboxViewOpen: Dispatch<SetStateAction<boolean>>;
  setNotesViewOpen: Dispatch<SetStateAction<boolean>>;
  invalidateLoadedSession: (sessionId: string) => void;
  activeTabId: string;
  setProjectTerminals: Dispatch<
    SetStateAction<import("../lib/projectTerminal").ProjectTerminalDock[]>
  >;
  setActiveTabId: Dispatch<SetStateAction<string>>;
  setComposerFocused: Dispatch<SetStateAction<boolean>>;
  setProjectCwd: Dispatch<SetStateAction<string>>;
  setRecents: Dispatch<SetStateAction<RecentProject[]>>;
  setHistory: Dispatch<SetStateAction<import("../lib/sessionStore").SessionSummary[]>>;
  setEditorNavigation: Dispatch<SetStateAction<EditorNavigationTarget | null>>;
  setProjectTerminalFocused: Dispatch<SetStateAction<boolean>>;
  projectOfTab: (id: string) => string | undefined;
  activateTab: (id: string, paneId?: string) => void;
  onFocusPane: (paneId: string) => void;
  appendTab: (tab: WorkspaceTab, cwd?: string) => void;
  persistSession: (session: Session | undefined) => void;
  refreshHistory: (cwd: string) => Promise<void>;
  readProjectReturnMemory: () => import("../lib/projectReturn").ProjectReturnMemory;
  focusOpenSession: (sessionId: string) => boolean;
  onSelectHistorySession: (sessionId: string) => Promise<void>;
}

export function useProjects(deps: ProjectsDeps) {
  const {
    activeTab,
    sessionsRef,
    tabsRef,
    activeTabIdRef,
    projectCwdRef,
    sidebarCwdRef,
    gitCwdRef,
    editorNavigationToken,
    turnGen,
    sessionLoads,
    pendingPersist,
    loadedSessionCache,
    lastPersisted,
    setSessions,
    setTabs,
    setDirtyFiles,
    setFileErrorCounts,
    setActiveTabId,
    setComposerFocused,
    setProjectCwd,
    setSearchViewOpen,
    setInboxViewOpen,
    setNotesViewOpen,
    setRecents,
    setEditorNavigation,
    projectOfTab,
    activateTab,
    onFocusPane,
    appendTab,
    persistSession,
    readProjectReturnMemory,
    invalidateLoadedSession,
    activeTabId,
    setProjectTerminals,
  } = deps;
  const onFocusDir = useCallback(
    (dir: FocusDir) => {
      if (!activeTab) return;
      const next = neighborLeafId(activeTab.layout, activeTab.focusedId, dir);
      if (next) onFocusPane(next);
    },
    [activeTab, onFocusPane],
  );

  const onRatio = useCallback(
    (tabId: string, splitId: string, index: number, ratio: number) => {
      setTabs((prev) =>
        prev.map((t) =>
          t.id === tabId
            ? { ...t, layout: setSplitRatio(t.layout, splitId, index, ratio) }
            : t,
        ),
      );
    },
    [],
  );

  const onCwdChange = useCallback(
    (sessionId: string, cwd: string) => {
      const normalized = normalizeProjectPath(cwd);
      const current = sessionsRef.current.find((s) => s.id === sessionId);
      const previous = current?.cwd;
      // Threads stay bound to their project. Switching from the composer opens a
      // new tab instead of retargeting the conversation.
      if (
        current &&
        previous &&
        looksLikeProject(previous) &&
        !sameProjectPath(previous, normalized) &&
        !isBlankSession(current)
      ) {
        setProjectCwd(normalized);
        setRecents(rememberProject(normalized));
        const session = newSession(
          current.harness,
          normalized,
          current.model,
          current.runtimeMode,
          current.modelSettings,
        );
        const tab = newTab(session.id);
        setSessions((prev) => [...prev, session]);
        appendTab(tab, normalized);
        setActiveTabId(tab.id);
        setComposerFocused(true);
        return;
      }
      if (
        previous &&
        !sameProjectPath(previous, normalized) &&
        previous !== "~"
      ) {
        void keepSessionChanges(sessionId, previous).catch(() => undefined);
      }
      setProjectCwd(normalized);
      setRecents(rememberProject(normalized));
      setSessions((prev) =>
        prev.map((s) =>
          s.id === sessionId
            ? {
                ...s,
                cwd: normalized,
                branch: undefined,
                worktreeCwd: undefined,
              }
            : s,
        ),
      );
      // The session's project just moved in place; a group only holds tabs that
      // share one project, so drop this tab out if it no longer matches.
      setTabs((prev) => {
        const tab = prev.find((t) => leafIds(t.layout).includes(sessionId));
        // The tab's visible project follows its focused pane; a background
        // pane changing project doesn't change what the group check should see.
        if (!tab?.groupId || tab.focusedId !== sessionId) return prev;
        const newProject = projectName(normalized);
        const othersProject = tabGroupProject(
          prev.filter((t) => t.id !== tab.id),
          tab.groupId,
          projectOfTab,
        );
        if (othersProject && newProject && othersProject !== newProject) {
          return removeTabFromGroup(prev, tab.id);
        }
        return prev;
      });
      notifyReviewChanged(sessionId);
    },
    [appendTab, projectOfTab],
  );

  const onBranchChange = useCallback(
    (sessionId: string) => {
      notifyGitChanged();
      const current = sessionsRef.current.find((s) => s.id === sessionId);
      if (!current || (!current.branch && !current.worktreeCwd)) return;
      if (current.worktreeCwd && current.providerSessionId) {
        void forgetHarnessSession(current.harness, sessionId);
      }
      const next = {
        ...current,
        branch: undefined,
        worktreeCwd: undefined,
        ...(current.worktreeCwd ? { providerSessionId: undefined } : {}),
      };
      setSessions((prev) => prev.map((s) => (s.id === sessionId ? next : s)));
      persistSession(next);
      notifyReviewChanged(sessionId);
    },
    [persistSession],
  );

  const onSelectProject = useCallback(
    (path: string) => {
      setSearchViewOpen(false);
      setInboxViewOpen(false);
      setNotesViewOpen(false);
      const normalized = normalizeProjectPath(path);
      if (!looksLikeProject(normalized)) return;

      const activeWorkspace = tabsRef.current.find(
        (entry) => entry.id === activeTabIdRef.current,
      );
      const current = activeWorkspace
        ? sessionsRef.current.find(
            (session) => session.id === activeWorkspace.focusedId,
          )
        : undefined;
      const decision = planProjectReturn({
        memory: readProjectReturnMemory(),
        tabs: tabsRef.current,
        sessions: sessionsRef.current,
        activeTabId: activeTabIdRef.current,
        projectPath: normalized,
      });
      switch (decision.action) {
        case "keep":
          setProjectCwd(normalized);
          setRecents(rememberProject(normalized));
          return;
        case "reuse-blank":
          onCwdChange(decision.sessionId, normalized);
          return;
        case "activate":
          setProjectCwd(normalized);
          setRecents(rememberProject(normalized));
          activateTab(decision.tabId, decision.paneId);
          return;
        case "create":
          break;
        default: {
          const exhaustive: never = decision;
          return exhaustive;
        }
      }

      const seed = current ?? sessionsRef.current[0];
      const session = newSession(
        seed?.harness ?? "claude",
        normalized,
        seed?.model,
        seed?.runtimeMode,
        seed?.modelSettings,
      );
      const tab = newTab(session.id);
      setProjectCwd(normalized);
      setRecents(rememberProject(normalized));
      setSessions((prev) => [...prev, session]);
      appendTab(tab, normalized);
      setActiveTabId(tab.id);
      setComposerFocused(true);
    },
    [activateTab, appendTab, onCwdChange, readProjectReturnMemory],
  );

  const pickProject = useCallback(async () => {
    const path = await pickFolder();
    if (path) onSelectProject(path);
  }, [onSelectProject]);

  const onPlaceSessionInFolder = useCallback(
    (sessionId: string, target: SessionFolderTarget) => {
      const source = sessionsRef.current.find(
        (session) => session.id === sessionId,
      );
      if (!source || !looksLikeProject(source.cwd)) return;
      const folders = loadSessionFolders(source.cwd);
      if (
        target.kind === "existing" &&
        !folders.some((folder) => folder.id === target.folderId)
      ) {
        return;
      }
      saveSessionFolders(
        source.cwd,
        placeSessionInFolder(folders, sessionId, target),
      );
    },
    [],
  );

  const onRemoveProject = useCallback(
    (path: string, options: { purgeData: boolean }) => {
      const normalized = normalizeProjectPath(path);
      const wasCurrent = sameProjectPath(projectCwdRef.current, normalized);
      const remaining = options.purgeData
        ? forgetProject(normalized)
        : archiveProject(normalized);
      setRecents(remaining);

      const tabs = tabsRef.current;
      const sessions = sessionsRef.current;
      const projectTabs = filterTabsForProject(tabs, sessions, normalized);
      const projectTabIds = new Set(projectTabs.map((tab) => tab.id));
      const projectSessions = sessions.filter((session) =>
        sameProjectPath(session.cwd, normalized),
      );
      const projectSessionIds = new Set(
        projectSessions.map((session) => session.id),
      );

      if (options.purgeData) {
        const cachedOrLoading = new Set([
          ...loadedSessionCache.current.keys(),
          ...sessionLoads.current.keys(),
        ]);
        for (const sessionId of cachedOrLoading) {
          invalidateLoadedSession(sessionId);
        }
        for (const session of projectSessions) {
          pendingPersist.current.delete(session.id);
          if (session.busy) {
            turnGen.current.set(
              session.id,
              (turnGen.current.get(session.id) ?? 0) + 1,
            );
            for (const id of sessionChildHarnesses(session)) {
              void cancelHarnessTurn(id, session.id);
            }
          }
          for (const id of sessionChildHarnesses(session)) {
            void forgetHarnessSession(id, session.id);
          }
          lastPersisted.current.delete(session.id);
        }
        void removeProjectData(normalized);
      } else {
        for (const session of projectSessions) {
          if (session.busy) continue;
          if (shouldPersistSession(session)) {
            rememberLoadedSession(loadedSessionCache.current, session);
          }
          persistSession(session);
          pendingPersist.current.delete(session.id);
          for (const id of sessionChildHarnesses(session)) {
            void forgetHarnessSession(id, session.id);
          }
        }
      }

      let nextTabs = tabs.filter((tab) => !projectTabIds.has(tab.id));
      let nextSessions = sessions.filter((session) => {
        if (!projectSessionIds.has(session.id)) return true;
        return !options.purgeData && session.busy;
      });
      let nextActiveTabId = activeTabIdRef.current;

      if (nextTabs.length === 0) {
        const fallback = nextSessions[0];
        const session = newDefaultSession("~", fallback?.runtimeMode);
        const tab = newTab(session.id);
        nextSessions = [...nextSessions, session];
        nextTabs = [tab];
        nextActiveTabId = tab.id;
      } else if (projectTabIds.has(nextActiveTabId)) {
        nextActiveTabId = nextTabs[0]?.id ?? nextActiveTabId;
      }

      sessionsRef.current = nextSessions;
      tabsRef.current = nextTabs;
      activeTabIdRef.current = nextActiveTabId;
      setSessions(nextSessions);
      setTabs(nextTabs);
      if (nextActiveTabId !== activeTabId) {
        setActiveTabId(nextActiveTabId);
      }
      setDirtyFiles((prev) => {
        const updated = new Set(prev);
        for (const tab of projectTabs) {
          for (const file of [
            ...tab.editorPanes.flatMap((pane) => pane.files),
            ...(tab.terminalPanes ?? []).flatMap((pane) => pane.files),
          ]) {
            updated.delete(file.id);
          }
        }
        return updated;
      });
      setProjectTerminals((prev) =>
        prev.filter((dock) => !sameProjectPath(dock.projectPath, normalized)),
      );

      if (wasCurrent) {
        const next = remaining.find((item) => looksLikeProject(item.path));
        if (next) {
          onSelectProject(next.path);
          setProjectCwd(next.path);
        } else {
          setProjectCwd("~");
          setComposerFocused(true);
        }
      }
    },
    [activeTabId, invalidateLoadedSession, onSelectProject, persistSession],
  );

  const onRestoreProject = useCallback(
    (path: string) => {
      setRecents(rememberProject(path));
      onSelectProject(path);
    },
    [onSelectProject],
  );

  const onFileMoved = useCallback((from: string, to: string) => {
    invalidateProjectFiles();
    setTabs((prev) =>
      prev.map((tab) => {
        return {
          ...tab,
          editorPanes: tab.editorPanes.map((pane) => ({
            ...pane,
            files: pane.files.map((file) =>
              isFilesystemTab(file)
                ? { ...file, path: rebasePath(file.path, from, to) }
                : file,
            ),
          })),
        };
      }),
    );
  }, []);

  const onFileDeleted = useCallback((path: string) => {
    invalidateProjectFiles();
    const dropped = new Set<string>();
    for (const tab of tabsRef.current) {
      for (const pane of tab.editorPanes) {
        for (const file of pane.files) {
          if (isFilesystemTab(file) && isEqualOrInside(file.path, path)) {
            dropped.add(file.id);
          }
        }
      }
    }
    setTabs((prev) =>
      prev.map((tab) =>
        dropOpenFiles(tab, (filePath) => isEqualOrInside(filePath, path)),
      ),
    );
    if (dropped.size === 0) return;
    setDirtyFiles((prev) => {
      const next = new Set(prev);
      for (const id of dropped) next.delete(id);
      return next;
    });
  }, []);

  const onOpenFile = useCallback<OpenFileFn>(
    (path, navigation, options) => {
      void (async () => {
        const resolved = await resolveFileOpenRequest(
          gitCwdRef.current,
          path,
          options,
        );
        rememberOpenedFile(sidebarCwdRef.current, resolved);
        const tab = tabsRef.current.find((entry) => entry.id === activeTabId);
        if (!tab) return;
        const file = newFileTab(resolved, sidebarCwdRef.current);
        setTabs((prev) =>
          prev.map((entry) => {
            if (entry.id !== tab.id) return entry;
            const focusedSession = sessionsRef.current.find(
              (session) => session.id === entry.focusedId,
            );
            return openEditorTab(entry, file, {
              split: focusedSession?.blocks.length === 0 ? "left" : "right",
            });
          }),
        );
        if (navigation) {
          editorNavigationToken.current += 1;
          setEditorNavigation({
            path: resolved,
            ...navigation,
            token: editorNavigationToken.current,
          });
        }
        setComposerFocused(false);
      })();
    },
    [activeTabId],
  );

  const onOpenPlan = useCallback(
    (sessionId: string, blockId: string) => {
      const tab = tabsRef.current.find((entry) => entry.id === activeTabId);
      const session = sessionsRef.current.find(
        (entry) => entry.id === sessionId,
      );
      const block = session?.blocks.find((entry) => entry.id === blockId);
      if (!tab || !session || !block) return;
      const file = newPlanTab(
        session.id,
        block.id,
        planTitle(block.text),
        session.cwd,
      );
      setTabs((prev) =>
        prev.map((entry) =>
          entry.id === tab.id ? openEditorTab(entry, file) : entry,
        ),
      );
      setComposerFocused(false);
    },
    [activeTabId],
  );

  const onFileDirtyChange = useCallback((fileId: string, dirty: boolean) => {
    setDirtyFiles((prev) => {
      if (prev.has(fileId) === dirty) return prev;
      const next = new Set(prev);
      if (dirty) next.add(fileId);
      else next.delete(fileId);
      return next;
    });
  }, []);

  /** The editor reports 0 as it unmounts, so closed tabs drop out on their own. */
  const onFileErrorCountChange = useCallback(
    (fileId: string, count: number) => {
      setFileErrorCounts((prev) => {
        if ((prev.get(fileId) ?? 0) === count) return prev;
        const next = new Map(prev);
        if (count > 0) next.set(fileId, count);
        else next.delete(fileId);
        return next;
      });
    },
    [],
  );

  const onSelectFileSurface = useCallback((paneId: string, fileId: string) => {
    setTabs((prev) =>
      prev.map((tab) => {
        const found = findSurfacePane(tab, paneId);
        if (!found) return tab;
        return withSurfacePanes(
          { ...tab, focusedId: paneId },
          found.kind,
          surfacePanes(tab, found.kind).map((pane) =>
            pane.id === paneId ? { ...pane, activeFileId: fileId } : pane,
          ),
        );
      }),
    );
    setComposerFocused(false);
  }, []);

  return {
    onPlaceSessionInFolder,
    onFileDirtyChange,
    onFileErrorCountChange,
    onSelectFileSurface,
    onFocusDir,
    onRatio,
    onCwdChange,
    onBranchChange,
    onSelectProject,
    pickProject,
    onRemoveProject,
    onRestoreProject,
    onFileMoved,
    onFileDeleted,
    onOpenFile,
    onOpenPlan,
  };
}
