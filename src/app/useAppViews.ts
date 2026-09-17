import {
  useCallback,
  useEffect,
  useRef,
  type Dispatch,
  type MutableRefObject,
  type SetStateAction,
} from "react";
import {
  saveProjectRailOpen,
  type SidebarTabId,
} from "../lib/appearance";
import {
  loadNotesEnabled,
  saveSettingsSection,
  type SettingsSectionId,
} from "../lib/settings";
import type { SettingsAnchor } from "../surfaces/SettingsView";
import type { ConnectableInboxSource } from "../lib/inboxFilters";
import type { LinkedWorkItem, Session } from "../lib/session";
import type { SessionSummary } from "../lib/sessionStore";

export interface LinkedWorkItemPanelState {
  item: LinkedWorkItem;
  sessionId: string;
  cwd: string;
}

export interface AppViewsDeps {
  history: SessionSummary[];
  sidebarCwd: string;
  sidebarTab: SidebarTabId;
  sessionsRef: MutableRefObject<Session[]>;
  searchViewOpen: boolean;
  inboxViewOpen: boolean;
  notesViewOpen: boolean;
  settingsOpen: boolean;
  dockVisible: boolean;
  setProjectRailOpen: Dispatch<SetStateAction<boolean>>;
  setSidebarTab: Dispatch<SetStateAction<SidebarTabId>>;
  setFilesSearchOpen: Dispatch<SetStateAction<boolean>>;
  setSearchFocusToken: Dispatch<SetStateAction<number>>;
  setSearchViewOpen: Dispatch<SetStateAction<boolean>>;
  setSearchViewFocusToken: Dispatch<SetStateAction<number>>;
  setInboxViewOpen: Dispatch<SetStateAction<boolean>>;
  setLinkedWorkItemPanels: Dispatch<
    SetStateAction<ReadonlyMap<string, LinkedWorkItemPanelState>>
  >;
  setNotesViewOpen: Dispatch<SetStateAction<boolean>>;
  setSettingsOpen: Dispatch<SetStateAction<boolean>>;
  setSettingsSection: Dispatch<SetStateAction<SettingsSectionId>>;
  setSettingsAnchor: Dispatch<SetStateAction<SettingsAnchor | null>>;
  setNotificationProjectPath: Dispatch<SetStateAction<string | null>>;
  setNotificationSettingsRequest: Dispatch<SetStateAction<number>>;
  setFilePickerOpen: Dispatch<SetStateAction<boolean>>;
  setProjectTerminalFocused: Dispatch<SetStateAction<boolean>>;
  onSelectHistorySession: (sessionId: string) => Promise<void>;
  onVisitBack: () => void;
  onVisitForward: () => void;
}

export function useAppViews(deps: AppViewsDeps) {
  const {
    history,
    sidebarCwd,
    sidebarTab,
    sessionsRef,
    searchViewOpen,
    inboxViewOpen,
    notesViewOpen,
    settingsOpen,
    dockVisible,
    setProjectRailOpen,
    setSidebarTab,
    setFilesSearchOpen,
    setSearchFocusToken,
    setSearchViewOpen,
    setSearchViewFocusToken,
    setInboxViewOpen,
    setLinkedWorkItemPanels,
    setNotesViewOpen,
    setSettingsOpen,
    setSettingsSection,
    setSettingsAnchor,
    setNotificationProjectPath,
    setNotificationSettingsRequest,
    setFilePickerOpen,
    setProjectTerminalFocused,
    onSelectHistorySession,
    onVisitBack,
    onVisitForward,
  } = deps;
  const linkedWorkItemPanelRequest = useRef(0);
  const closeLinkedWorkItemPanel = useCallback((sessionId: string) => {
    linkedWorkItemPanelRequest.current += 1;
    setLinkedWorkItemPanels((current) => {
      if (!current.has(sessionId)) return current;
      const next = new Map(current);
      next.delete(sessionId);
      return next;
    });
  }, []);
  const onToggleSidebar = useCallback(() => {
    setProjectRailOpen((open) => {
      const next = !open;
      saveProjectRailOpen(next);
      return next;
    });
  }, []);

  const onToggleProjectRail = useCallback(() => {
    setProjectRailOpen((open) => {
      const next = !open;
      saveProjectRailOpen(next);
      return next;
    });
  }, []);

  const onGoToFile = useCallback(() => {
    setSearchViewOpen(false);
    setInboxViewOpen(false);
    setNotesViewOpen(false);
    setFilePickerOpen(true);
  }, []);

  const onFindInProject = useCallback(() => {
    setSearchViewOpen(false);
    setInboxViewOpen(false);
    setNotesViewOpen(false);
    setSidebarTab("files");
    setFilesSearchOpen(true);
    setSearchFocusToken((token) => token + 1);
  }, []);

  const onOpenSearch = useCallback(() => {
    setFilePickerOpen(false);
    setSettingsOpen(false);
    setInboxViewOpen(false);
    setNotesViewOpen(false);
    setSearchViewOpen(true);
    setSearchViewFocusToken((token) => token + 1);
  }, []);

  const onLeaveSearch = useCallback(() => {
    setSearchViewOpen(false);
  }, []);

  const onOpenInbox = useCallback(() => {
    setFilePickerOpen(false);
    setSettingsOpen(false);
    setSearchViewOpen(false);
    setNotesViewOpen(false);
    setInboxViewOpen(true);
  }, []);

  const onOpenLinkedWorkItem = useCallback(
    (item: LinkedWorkItem, sessionId: string) => {
      const request = linkedWorkItemPanelRequest.current + 1;
      linkedWorkItemPanelRequest.current = request;
      setFilePickerOpen(false);
      setSettingsOpen(false);
      setSearchViewOpen(false);
      setNotesViewOpen(false);
      setInboxViewOpen(false);
      const cwd =
        sessionsRef.current.find((session) => session.id === sessionId)?.cwd ??
        history.find((session) => session.id === sessionId)?.cwd ??
        sidebarCwd;
      void onSelectHistorySession(sessionId).then(() => {
        if (linkedWorkItemPanelRequest.current !== request) return;
        if (!sessionsRef.current.some((session) => session.id === sessionId)) {
          return;
        }
        setLinkedWorkItemPanels((current) => {
          const next = new Map(current);
          // Reinsert the panel so it wins if this workspace tab contains
          // multiple sessions with remembered panels.
          next.delete(sessionId);
          next.set(sessionId, { item, sessionId, cwd });
          return next;
        });
      });
    },
    [history, onSelectHistorySession, sidebarCwd],
  );

  const onLeaveInbox = useCallback(() => {
    setInboxViewOpen(false);
  }, []);

  const onOpenInboxSession = useCallback(
    (sessionId: string) => {
      setInboxViewOpen(false);
      setSidebarTab("sessions");
      void onSelectHistorySession(sessionId);
    },
    [onSelectHistorySession],
  );

  const onOpenNotes = useCallback(() => {
    if (!loadNotesEnabled()) return;
    setFilePickerOpen(false);
    setSettingsOpen(false);
    setSearchViewOpen(false);
    setInboxViewOpen(false);
    setNotesViewOpen(true);
  }, []);

  const onLeaveNotes = useCallback(() => {
    setNotesViewOpen(false);
  }, []);

  const openSettings = useCallback(
    (section?: SettingsSectionId, anchor?: SettingsAnchor) => {
      setFilePickerOpen(false);
      setSearchViewOpen(false);
      setInboxViewOpen(false);
      setNotesViewOpen(false);
      if (section) {
        setSettingsSection(section);
        saveSettingsSection(section);
      }
      setSettingsAnchor(anchor ?? null);
      setNotificationProjectPath(null);
      setSettingsOpen(true);
    },
    [],
  );

  const onOpenSettings = useCallback(() => openSettings(), [openSettings]);

  const onOpenNotificationSettings = useCallback((path?: string) => {
    openSettings("inbox", "project-notifications");
    setNotificationProjectPath(path ?? null);
    setNotificationSettingsRequest((request) => request + 1);
  }, [openSettings]);

  const onOpenInboxIntegrations = useCallback(
    (source: ConnectableInboxSource) => openSettings("inbox", source),
    [openSettings],
  );

  const onCloseSettings = useCallback(() => {
    setSettingsOpen(false);
  }, []);

  const onSelectSettingsSection = useCallback((section: SettingsSectionId) => {
    setSettingsSection(section);
    saveSettingsSection(section);
  }, []);

  const onOpenArchivedSession = useCallback(
    (sessionId: string) => {
      setSettingsOpen(false);
      void onSelectHistorySession(sessionId);
    },
    [onSelectHistorySession],
  );

  const onRailBack = useCallback(() => {
    if (settingsOpen) {
      setSettingsOpen(false);
      return;
    }
    if (searchViewOpen) {
      setSearchViewOpen(false);
      return;
    }
    if (inboxViewOpen) {
      setInboxViewOpen(false);
      return;
    }
    if (notesViewOpen) {
      setNotesViewOpen(false);
      return;
    }
    onVisitBack();
  }, [onVisitBack, searchViewOpen, settingsOpen, inboxViewOpen, notesViewOpen]);

  const onRailForward = useCallback(() => {
    setSearchViewOpen(false);
    setSettingsOpen(false);
    setInboxViewOpen(false);
    setNotesViewOpen(false);
    onVisitForward();
  }, [onVisitForward]);

  useEffect(() => {
    if (sidebarTab === "inbox") setSidebarTab("sessions");
  }, [sidebarTab]);

  useEffect(() => {
    if (!dockVisible) setProjectTerminalFocused(false);
  }, [dockVisible]);
  return {
    closeLinkedWorkItemPanel,
    onToggleSidebar,
    onToggleProjectRail,
    onGoToFile,
    onFindInProject,
    onOpenSearch,
    onLeaveSearch,
    onOpenInbox,
    onOpenLinkedWorkItem,
    onLeaveInbox,
    onOpenInboxSession,
    onOpenNotes,
    onLeaveNotes,
    openSettings,
    onOpenSettings,
    onOpenNotificationSettings,
    onOpenInboxIntegrations,
    onCloseSettings,
    onSelectSettingsSection,
    onOpenArchivedSession,
    onRailBack,
    onRailForward,
  };
}
