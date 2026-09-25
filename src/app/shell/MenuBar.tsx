import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useRef, useState } from "react";
import {
  ExplorerMenu,
  type ExplorerMenuItem,
} from "../../features/files/ui/ExplorerMenu";
import { ALT, MOD, SHIFT } from "../../platform/tauri/platform";
import { runUpdateFlow } from "../model/updater";
import {
  loadKeybindingOverrides,
  subscribeKeybindings,
} from "../../features/settings/model/settings";
import { quickComposerShortcutLabel } from "../../features/quick-composer/model/quickComposerShortcut";

type MenuKey = "file" | "view" | "terminal";

type Props = {
  onNew: () => void;
  onNewTerminal?: () => void;
  onToggleTerminal?: () => void;
  onGoToFile?: () => void;
  onToggleSidebar: () => void;
  onToggleSessionSidebar: () => void;
  onShowSourceControl?: () => void;
  onCloseCurrentTab?: () => void;
  onCloseOtherTabs?: () => void;
  onCloseAllTabs?: () => void;
  onPickProject?: () => void;
  onFindInProject?: () => void;
  onSearch?: () => void;
  onOpenInbox?: () => void;
  onOpenNotes?: () => void;
  onZoomIn?: () => void;
  onZoomOut?: () => void;
  onZoomReset?: () => void;
};

export function MenuBar({
  onNew,
  onNewTerminal,
  onToggleTerminal,
  onGoToFile,
  onToggleSidebar,
  onToggleSessionSidebar,
  onShowSourceControl,
  onCloseCurrentTab,
  onCloseOtherTabs,
  onCloseAllTabs,
  onPickProject,
  onFindInProject,
  onSearch,
  onOpenInbox,
  onOpenNotes,
  onZoomIn,
  onZoomOut,
  onZoomReset,
}: Props) {
  const [open, setOpen] = useState(false);
  const [activeMenu, setActiveMenu] = useState<MenuKey | null>(null);
  const [menuAnchor, setMenuAnchor] = useState<{ x: number; y: number } | null>(
    null,
  );
  const [keybindingOverrides, setKeybindingOverrides] = useState(
    loadKeybindingOverrides,
  );
  const barRef = useRef<HTMLDivElement>(null);

  useEffect(
    () =>
      subscribeKeybindings(() =>
        setKeybindingOverrides(loadKeybindingOverrides()),
      ),
    [],
  );

  const shortcut = (command: string, keys: string) => {
    const override = keybindingOverrides[command];
    if (override?.disabled) return undefined;
    return override?.shortcut
      ? quickComposerShortcutLabel(override.shortcut)
      : keys;
  };

  // Toggle with standalone Alt key tap
  useEffect(() => {
    let altPressedAlone = false;

    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Alt") {
        altPressedAlone = true;
      } else if (altPressedAlone) {
        altPressedAlone = false;
      }
    };

    const onKeyUp = (e: KeyboardEvent) => {
      if (e.key === "Alt" && altPressedAlone) {
        setOpen((prev) => {
          if (prev) {
            setActiveMenu(null);
            setMenuAnchor(null);
            return false;
          }
          return true;
        });
        altPressedAlone = false;
      }
    };

    const onBlur = () => {
      altPressedAlone = false;
    };

    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("keyup", onKeyUp);
    window.addEventListener("blur", onBlur);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("keyup", onKeyUp);
      window.removeEventListener("blur", onBlur);
    };
  }, []);

  const openDropdown = useCallback((key: MenuKey, target: HTMLElement) => {
    const rect = target.getBoundingClientRect();
    setActiveMenu(key);
    setMenuAnchor({ x: rect.left, y: rect.bottom + 2 });
  }, []);

  const closeMenu = useCallback(() => {
    setActiveMenu(null);
    setMenuAnchor(null);
  }, []);

  const handlePick = useCallback(
    (id: string) => {
      closeMenu();
      setOpen(false);

      switch (id) {
        case "new_tab":
          onNew();
          break;
        case "new_terminal":
          onNewTerminal?.();
          break;
        case "toggle_terminal":
          onToggleTerminal?.();
          break;
        case "new_window":
          void invoke("open_new_window").catch(() => {});
          break;
        case "open_project":
          onPickProject?.();
          break;
        case "open_search":
          onSearch?.();
          break;
        case "open_inbox":
          onOpenInbox?.();
          break;
        case "open_notes":
          onOpenNotes?.();
          break;
        case "go_to_file":
          onGoToFile?.();
          break;
        case "find_in_project":
          onFindInProject?.();
          break;
        case "close_tab":
          onCloseCurrentTab?.();
          break;
        case "close_other_tabs":
          onCloseOtherTabs?.();
          break;
        case "close_all_tabs":
          onCloseAllTabs?.();
          break;
        case "toggle_sidebar":
          onToggleSidebar();
          break;
        case "toggle_session_sidebar":
          onToggleSessionSidebar();
          break;
        case "open_model_picker":
          window.dispatchEvent(new Event("open_model_picker"));
          break;
        case "toggle_diff":
          onShowSourceControl?.();
          break;
        case "check_for_updates":
          void runUpdateFlow(true);
          break;
        case "zoom_in":
          onZoomIn?.();
          break;
        case "zoom_out":
          onZoomOut?.();
          break;
        case "zoom_reset":
          onZoomReset?.();
          break;
      }
    },
    [
      closeMenu,
      onCloseCurrentTab,
      onCloseOtherTabs,
      onCloseAllTabs,
      onFindInProject,
      onGoToFile,
      onNew,
      onNewTerminal,
      onToggleTerminal,
      onPickProject,
      onSearch,
      onOpenInbox,
      onOpenNotes,
      onShowSourceControl,
      onToggleSidebar,
      onToggleSessionSidebar,
      onZoomIn,
      onZoomOut,
      onZoomReset,
    ],
  );

  const getMenuItems = (key: MenuKey): ExplorerMenuItem[] => {
    switch (key) {
      case "file":
        return [
          {
            kind: "item",
            id: "new_tab",
            label: "New Tab",
            shortcut: shortcut("Tab: New", `${MOD}T`),
          },
          {
            kind: "item",
            id: "new_terminal",
            label: "New Terminal",
            shortcut: shortcut("Terminal: New", `${MOD}\``),
          },
          {
            kind: "item",
            id: "new_window",
            label: "New Window",
            shortcut: shortcut("App: New Window", `${MOD}${SHIFT}N`),
          },
          { kind: "sep" },
          {
            kind: "item",
            id: "open_project",
            label: "Open Project…",
            shortcut: shortcut("App: Open Project", `${MOD}O`),
          },
          {
            kind: "item",
            id: "open_search",
            label: "Search…",
            shortcut: shortcut("App: Search", `${MOD}K`),
          },
          {
            kind: "item",
            id: "go_to_file",
            label: "Go to File…",
            shortcut: shortcut("App: Go to File", `${MOD}P`),
          },
          {
            kind: "item",
            id: "find_in_project",
            label: "Find in Files…",
            shortcut: shortcut("App: Find in Files", `${MOD}${SHIFT}F`),
          },
          { kind: "sep" },
          {
            kind: "item",
            id: "close_tab",
            label: "Close Pane",
            shortcut: shortcut("Pane: Close", `${MOD}W`),
          },
          {
            kind: "item",
            id: "close_other_tabs",
            label: "Close Other Tabs",
            shortcut: shortcut("Tab: Close Others", `${MOD}${ALT}T`),
          },
          {
            kind: "item",
            id: "close_all_tabs",
            label: "Close All Tabs",
            shortcut: shortcut("Tab: Close All", `${MOD}${SHIFT}W`),
          },
          { kind: "sep" },
          {
            kind: "item",
            id: "check_for_updates",
            label: "Check for Updates…",
          },
        ];
      case "view":
        return [
          {
            kind: "item",
            id: "toggle_sidebar",
            label: "Toggle Sidebar",
            shortcut: shortcut("App: Toggle Sidebar", `${MOD}B`),
          },
          {
            kind: "item",
            id: "toggle_session_sidebar",
            label: "Toggle Session Sidebar",
            shortcut: shortcut(
              "App: Toggle Session Sidebar",
              `${MOD}${SHIFT}B`,
            ),
          },
          { kind: "item", id: "open_inbox", label: "Inbox" },
          ...(onOpenNotes
            ? [{ kind: "item" as const, id: "open_notes", label: "Notes" }]
            : []),
          {
            kind: "item",
            id: "toggle_terminal",
            label: "Toggle Terminal",
            shortcut: shortcut("Terminal: Toggle Dock", `${MOD}J`),
          },
          {
            kind: "item",
            id: "open_model_picker",
            label: "Switch Model…",
            shortcut: shortcut("App: Switch Model", `${MOD}.`),
          },
          { kind: "item", id: "toggle_diff", label: "Toggle Changes" },
          { kind: "sep" },
          {
            kind: "item",
            id: "zoom_in",
            label: "Zoom In",
            shortcut: shortcut("View: Zoom In", `${MOD}+`),
          },
          {
            kind: "item",
            id: "zoom_out",
            label: "Zoom Out",
            shortcut: shortcut("View: Zoom Out", `${MOD}-`),
          },
          {
            kind: "item",
            id: "zoom_reset",
            label: "Reset Zoom",
            shortcut: shortcut("View: Reset Zoom", `${MOD}0`),
          },
        ];
      case "terminal":
        return [
          {
            kind: "item",
            id: "new_terminal",
            label: "New Terminal",
            shortcut: shortcut("Terminal: New", `${MOD}\``),
          },
          {
            kind: "item",
            id: "toggle_terminal",
            label: "Toggle Terminal",
            shortcut: shortcut("Terminal: Toggle Dock", `${MOD}J`),
          },
        ];
    }
  };

  if (!open && !activeMenu) {
    return null;
  }

  const MENUS: { key: MenuKey; label: string }[] = [
    { key: "file", label: "File" },
    { key: "view", label: "View" },
    { key: "terminal", label: "Terminal" },
  ];

  return (
    <div
      ref={barRef}
      className="flex h-7 shrink-0 items-center gap-0.5 border-b border-stroke bg-content/5 px-2 text-[12px]"
      data-tauri-drag-region="false"
    >
      {MENUS.map(({ key, label }) => {
        const isActive = activeMenu === key;
        return (
          <button
            key={key}
            type="button"
            data-tauri-drag-region="false"
            onClick={(e) => {
              if (isActive) {
                closeMenu();
              } else {
                openDropdown(key, e.currentTarget);
              }
            }}
            onMouseEnter={(e) => {
              if (activeMenu && activeMenu !== key) {
                openDropdown(key, e.currentTarget);
              }
            }}
            className={`rounded px-2 py-0.5 transition-colors ${
              isActive
                ? "bg-selection-hover text-content"
                : "text-content/70 hover:bg-content/10 hover:text-content"
            }`}
          >
            {label}
          </button>
        );
      })}

      {activeMenu && menuAnchor ? (
        <ExplorerMenu
          x={menuAnchor.x}
          y={menuAnchor.y}
          items={getMenuItems(activeMenu)}
          ariaLabel={`${activeMenu} menu`}
          onPick={handlePick}
          onClose={closeMenu}
        />
      ) : null}
    </div>
  );
}
