import {
  useCallback,
  useEffect,
  useRef,
  type Dispatch,
  type MutableRefObject,
  type SetStateAction,
} from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";
import {
  adjacentItemId,
  shouldHandleListNavigation,
  tabCommand,
} from "../lib/tabKeys";
import {
  loadRecents,
  normalizeProjectPath,
  projectRailItems,
  sameProjectPath,
} from "../lib/recents";
import {
  UI_SCALE_DEFAULT,
  applyUiScale,
  loadUiScale,
  saveUiScale,
  uiScaleCommand,
  zoomInUiScale,
  zoomOutUiScale,
} from "../lib/uiScale";
import {
  handleEditorFindKey,
  openFindInActiveEditor,
} from "../surfaces/editorSearch";
import { runUpdateFlow } from "../lib/updater";
import { NOTIFICATION_CLICK_EVENT } from "../lib/notifications";
import type { Session } from "../lib/session";
import type { WorkspaceTab } from "../lib/layout";

export interface ShortcutAction {
  onNew: () => string;
  onArchiveFocusedSession: (event: KeyboardEvent) => void;
  onCloseOtherTabs: () => void;
  onCloseAllTabs: () => void;
  onClosePane: () => void;
  onNext: () => void;
  onPrev: () => void;
  onVisitBack: () => void;
  onVisitForward: () => void;
  onActivate: (slot: number) => void;
  onSplit: (dir: import("../lib/layout").SplitDir) => void;
  onFocusDir: (dir: import("../lib/layout").FocusDir) => void;
  onToggleSidebar: () => void;
  onGoToFile: () => void;
  onFindInProject: () => void;
  onOpenSearch: () => void;
  onOpenInbox: () => void;
  onOpenNotes: () => void;
  pickProject: () => void;
  onNewTerminal: () => void;
  onNewTerminalTab: () => void;
  onToggleProjectTerminal: () => void;
  onOpenApprovalSession: (sessionId: string) => void;
  openSettings: (section?: import("../lib/settings").SettingsSectionId) => void;
}

export interface AppShortcutsDeps {
  tabs: WorkspaceTab[];
  tabsRef: MutableRefObject<WorkspaceTab[]>;
  sessionsRef: MutableRefObject<Session[]>;
  activeTabIdRef: MutableRefObject<string>;
  projectCwdRef: MutableRefObject<string>;
  filePickerOpenRef: MutableRefObject<boolean>;
  searchViewOpenRef: MutableRefObject<boolean>;
  inboxViewOpenRef: MutableRefObject<boolean>;
  notesViewOpenRef: MutableRefObject<boolean>;
  settingsOpenRef: MutableRefObject<boolean>;
  whatsNewVersionRef: MutableRefObject<string | null>;
  sessionNavigationIdsRef: MutableRefObject<readonly string[]>;
  setSidebarTab: Dispatch<SetStateAction<import("../lib/appearance").SidebarTabId>>;
  onSelectHistorySession: (sessionId: string) => Promise<void>;
  onSelectProject: (path: string) => void;
  actions: ShortcutAction;
}

export function useAppShortcuts(deps: AppShortcutsDeps) {
  const {
    tabsRef,
    sessionsRef,
    activeTabIdRef,
    projectCwdRef,
    filePickerOpenRef,
    searchViewOpenRef,
    inboxViewOpenRef,
    notesViewOpenRef,
    settingsOpenRef,
    whatsNewVersionRef,
    sessionNavigationIdsRef,
    onSelectHistorySession,
    onSelectProject,
  } = deps;
  const {
    onNew,
    onArchiveFocusedSession,
    onCloseOtherTabs,
    onCloseAllTabs,
    onClosePane,
    onNext,
    onPrev,
    onVisitBack,
    onVisitForward,
    onActivate,
    onSplit,
    onFocusDir,
    onToggleSidebar,
    onGoToFile,
    onFindInProject,
    onOpenSearch,
    onOpenInbox,
    onOpenNotes,
    pickProject,
    onNewTerminal,
    onNewTerminalTab,
    onToggleProjectTerminal,
    onOpenApprovalSession,
    openSettings,
  } = deps.actions;
  const onSessionNavigationOrder = useCallback((ids: readonly string[]) => {
    sessionNavigationIdsRef.current = ids;
  }, []);

  const onNavigateSessionList = useCallback(
    (delta: number) => {
      const activeWorkspace = tabsRef.current.find(
        (entry) => entry.id === activeTabIdRef.current,
      );
      if (!activeWorkspace || activeWorkspace.diffFocused) return;
      const current = sessionsRef.current.find(
        (session) => session.id === activeWorkspace.focusedId,
      );
      if (!current) return;

      const next = adjacentItemId(
        sessionNavigationIdsRef.current,
        current.id,
        delta,
      );
      if (!next || next === current.id) return;
      void onSelectHistorySession(next);
    },
    [onSelectHistorySession],
  );

  const onNavigateProjectList = useCallback(
    (delta: number) => {
      const current = normalizeProjectPath(projectCwdRef.current);
      const ids = projectRailItems(loadRecents(), current).map(
        (project) => project.path,
      );
      const next = adjacentItemId(ids, current, delta);
      if (!next || sameProjectPath(next, current)) return;
      onSelectProject(next);
    },
    [onSelectProject],
  );

  const actions = useRef({
    onNew,
    onArchiveFocusedSession,
    onCloseOtherTabs,
    onCloseAllTabs,
    onClosePane,
    onNext,
    onPrev,
    onVisitBack,
    onVisitForward,
    onActivate,
    onSplit,
    onFocusDir,
    onToggleSidebar,
    onGoToFile,
    onFindInProject,
    onOpenSearch,
    onOpenInbox,
    onOpenNotes,
    pickProject,
    onNewTerminal,
    onNewTerminalTab,
    onToggleProjectTerminal,
    onNavigateSessionList,
    onNavigateProjectList,
    openSettings,
    onOpenApprovalSession,
  });
  actions.current = {
    onNew,
    onArchiveFocusedSession,
    onCloseOtherTabs,
    onCloseAllTabs,
    onClosePane,
    onNext,
    onPrev,
    onVisitBack,
    onVisitForward,
    onActivate,
    onSplit,
    onFocusDir,
    onToggleSidebar,
    onGoToFile,
    onFindInProject,
    onOpenSearch,
    onOpenInbox,
    onOpenNotes,
    pickProject,
    onNewTerminal,
    onNewTerminalTab,
    onToggleProjectTerminal,
    onNavigateSessionList,
    onNavigateProjectList,
    openSettings,
    onOpenApprovalSession,
  };

  const debounce = useRef({ name: "", at: 0 });
  const run = useCallback((name: string, fn: () => void) => {
    const now = performance.now();
    if (name === debounce.current.name && now - debounce.current.at < 80)
      return;
    debounce.current = { name, at: now };
    fn();
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      // Browser-standard UI zoom. Runs before tabCommand and always applies —
      // even in inputs and the terminal — so Ctrl/Cmd + - 0 behave like a browser.
      if ((e.metaKey || e.ctrlKey) && !e.altKey && !e.isComposing) {
        const zoom = uiScaleCommand(e);
        if (zoom) {
          e.preventDefault();
          e.stopPropagation();
          if (zoom === "zoom-in") {
            const next = saveUiScale(zoomInUiScale(loadUiScale()));
            void applyUiScale(next);
          } else if (zoom === "zoom-out") {
            const next = saveUiScale(zoomOutUiScale(loadUiScale()));
            void applyUiScale(next);
          } else {
            saveUiScale(UI_SCALE_DEFAULT);
            void applyUiScale(UI_SCALE_DEFAULT);
          }
          return;
        }
      }
      const cmd = tabCommand(e);
      if (cmd) {
        if (cmd === "archive-session") {
          actions.current.onArchiveFocusedSession(e);
          return;
        }
        const target = e.target instanceof Element ? e.target : null;
        const listNavigation =
          cmd === "prev-session" ||
          cmd === "next-session" ||
          cmd === "prev-project" ||
          cmd === "next-project";
        if (listNavigation) {
          const blockedTarget = Boolean(
            target?.closest(
              'input, textarea, select, [contenteditable="true"], .cm-editor, .monocode-terminal, [role="dialog"], [data-model-picker], [data-file-picker], [data-branch-picker], [data-skill-picker], [data-mention-picker], [data-app-search]',
            ),
          );
          const emptyComposerTarget = Boolean(
            target?.matches('textarea[data-composer-empty="true"]'),
          );
          const surfaceOpen =
            searchViewOpenRef.current ||
            inboxViewOpenRef.current ||
            notesViewOpenRef.current ||
            settingsOpenRef.current ||
            filePickerOpenRef.current ||
            Boolean(whatsNewVersionRef.current);
          if (
            !shouldHandleListNavigation({
              blockedTarget,
              emptyComposerTarget,
              surfaceOpen,
            })
          ) {
            return;
          }
        }
        if (
          target?.closest(".monocode-terminal") &&
          e.ctrlKey &&
          !e.metaKey &&
          (cmd === "back" ||
            cmd === "forward" ||
            /Mac|iPhone|iPad/.test(navigator.platform))
        ) {
          return;
        }
        if (
          (cmd === "split-right" || cmd === "split-down") &&
          target?.closest(".cm-editor")
        ) {
          return;
        }
        const inPicker =
          target &&
          target.closest(
            "[data-model-picker], [data-file-picker], [data-branch-picker], [data-skill-picker], [data-mention-picker], [data-app-search]",
          );
        if (inPicker && typeof cmd === "object" && "activate" in cmd) {
          return;
        }
        e.preventDefault();
        e.stopPropagation();
        const a = actions.current;
        if (cmd === "new") run("new", a.onNew);
        else if (cmd === "close-others")
          run("close-others", a.onCloseOtherTabs);
        else if (cmd === "close-all") run("close-all", a.onCloseAllTabs);
        else if (cmd === "close") run("close", a.onClosePane);
        else if (cmd === "next") run("next", a.onNext);
        else if (cmd === "prev") run("prev", a.onPrev);
        else if (cmd === "back") run("back", a.onVisitBack);
        else if (cmd === "forward") run("forward", a.onVisitForward);
        else if (cmd === "split-right")
          run("split-right", () => a.onSplit("right"));
        else if (cmd === "split-down")
          run("split-down", () => a.onSplit("down"));
        else if (cmd === "new-terminal") run("new-terminal", a.onNewTerminal);
        else if (cmd === "new-terminal-tab")
          run("new-terminal-tab", a.onNewTerminalTab);
        else if (cmd === "toggle-terminal")
          run("toggle-terminal", a.onToggleProjectTerminal);
        else if (cmd === "prev-session")
          run("prev-session", () => a.onNavigateSessionList(-1));
        else if (cmd === "next-session")
          run("next-session", () => a.onNavigateSessionList(1));
        else if (cmd === "prev-project")
          run("prev-project", () => a.onNavigateProjectList(-1));
        else if (cmd === "next-project")
          run("next-project", () => a.onNavigateProjectList(1));
        else if ("focus" in cmd)
          run(`focus-${cmd.focus}`, () => a.onFocusDir(cmd.focus));
        else run(`activate-${cmd.activate}`, () => a.onActivate(cmd.activate));
        return;
      }
      if (
        !searchViewOpenRef.current &&
        !inboxViewOpenRef.current &&
        !notesViewOpenRef.current &&
        handleEditorFindKey(e)
      ) {
        e.stopPropagation();
        return;
      }
      const mod = e.metaKey || e.ctrlKey;
      if (mod && !e.altKey && !e.shiftKey && e.key.toLowerCase() === "b") {
        e.preventDefault();
        e.stopPropagation();
        run("toggle_sidebar", actions.current.onToggleSidebar);
        return;
      }
      if (mod && !e.altKey && !e.shiftKey && e.key.toLowerCase() === "p") {
        e.preventDefault();
        e.stopPropagation();
        run("go_to_file", actions.current.onGoToFile);
        return;
      }
      if (mod && !e.altKey && !e.shiftKey && e.key.toLowerCase() === "k") {
        const target = e.target instanceof Element ? e.target : null;
        if (target?.closest(".monocode-terminal") && e.ctrlKey && !e.metaKey) {
          return;
        }
        e.preventDefault();
        e.stopPropagation();
        run("open_search", actions.current.onOpenSearch);
        return;
      }
      if (mod && !e.altKey && !e.shiftKey && e.key === ",") {
        e.preventDefault();
        e.stopPropagation();
        run("open_settings", () => actions.current.openSettings());
        return;
      }
      if (mod && e.shiftKey && !e.altKey && e.key.toLowerCase() === "f") {
        e.preventDefault();
        e.stopPropagation();
        run("find_in_project", actions.current.onFindInProject);
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [run]);

  useEffect(() => {
    const unlisten: Array<Promise<() => void>> = [
      listen("new_tab", () => run("new", actions.current.onNew)),
      listen("close_other_tabs", () =>
        run("close-others", actions.current.onCloseOtherTabs),
      ),
      listen("close_all_tabs", () =>
        run("close-all", actions.current.onCloseAllTabs),
      ),
      listen("close_tab", () => run("close", actions.current.onClosePane)),
      listen("next_tab", () => run("next", actions.current.onNext)),
      listen("prev_tab", () => run("prev", actions.current.onPrev)),
      listen("back_tab", () => run("back", actions.current.onVisitBack)),
      listen("forward_tab", () =>
        run("forward", actions.current.onVisitForward),
      ),
      listen("split_right", () =>
        run("split-right", () => actions.current.onSplit("right")),
      ),
      listen("split_down", () =>
        run("split-down", () => actions.current.onSplit("down")),
      ),
      listen("new_terminal", () =>
        run("new-terminal", actions.current.onNewTerminal),
      ),
      listen("new_terminal_tab", () =>
        run("new-terminal-tab", actions.current.onNewTerminalTab),
      ),
      listen("toggle_terminal", () =>
        run("toggle-terminal", actions.current.onToggleProjectTerminal),
      ),
      listen("focus_left", () =>
        run("focus-left", () => actions.current.onFocusDir("left")),
      ),
      listen("focus_right", () =>
        run("focus-right", () => actions.current.onFocusDir("right")),
      ),
      listen("focus_up", () =>
        run("focus-up", () => actions.current.onFocusDir("up")),
      ),
      listen("focus_down", () =>
        run("focus-down", () => actions.current.onFocusDir("down")),
      ),
      listen("toggle_sidebar", () =>
        run("toggle_sidebar", actions.current.onToggleSidebar),
      ),
      listen("open_project", () => {
        void actions.current.pickProject();
      }),
      listen("go_to_file", () => actions.current.onGoToFile()),
      listen("open_search", () => actions.current.onOpenSearch()),
      listen("open_inbox", () => actions.current.onOpenInbox()),
      listen("open_notes", () => actions.current.onOpenNotes()),
      listen("open_settings", () => actions.current.openSettings()),
      listen("check_for_updates", () => {
        void runUpdateFlow(true);
      }),
      listen("sidebar_opacity", () => {
        actions.current.openSettings("appearance");
      }),
      listen("find_in_project", () => actions.current.onFindInProject()),
      listen("find", () => {
        openFindInActiveEditor();
      }),
      listen("open_model_picker", () => {
        window.dispatchEvent(new Event("open_model_picker"));
      }),
      // Every window hears the click; only the one holding the session acts.
      listen<string>(NOTIFICATION_CLICK_EVENT, ({ payload: sessionId }) => {
        if (!sessionsRef.current.some((s) => s.id === sessionId)) return;
        const win = getCurrentWindow();
        // Windows leaves a minimized window minimized when it is only focused.
        void win
          .unminimize()
          .then(() => win.setFocus())
          .catch(() => {});
        actions.current.onOpenApprovalSession(sessionId);
      }),
      listen("zoom_in", () => {
        const next = zoomInUiScale(loadUiScale());
        saveUiScale(next);
        void applyUiScale(next);
      }),
      listen("zoom_out", () => {
        const next = zoomOutUiScale(loadUiScale());
        saveUiScale(next);
        void applyUiScale(next);
      }),
      listen("zoom_reset", () => {
        saveUiScale(UI_SCALE_DEFAULT);
        void applyUiScale(UI_SCALE_DEFAULT);
      }),
    ];
    return () => {
      void Promise.all(unlisten).then((fns) => fns.forEach((fn) => fn()));
    };
  }, [run]);

  return {
    onSessionNavigationOrder,
    onNavigateSessionList,
    onNavigateProjectList,
  };
}
