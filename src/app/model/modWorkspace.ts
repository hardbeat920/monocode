import {
  editorTabKey,
  isModTab,
  newModTab,
  openEditorTab,
  type ModTabSource,
  type WorkspaceTab,
} from "../../features/workspace/model/layout";

/**
 * Opens a mod's `Pane` view: focuses it wherever it is already open, else
 * opens it in the workspace tab `targetTabId`. `tabId` is the workspace tab
 * to activate, or null when `targetTabId` was not found.
 */
export function openModTab(
  tabs: WorkspaceTab[],
  targetTabId: string | null,
  mod: ModTabSource,
  cwd: string,
): { tabs: WorkspaceTab[]; tabId: string | null } {
  const file = newModTab(mod, cwd);
  const key = editorTabKey(file);
  const existing = tabs.find((tab) =>
    tab.editorPanes.some((pane) =>
      pane.files.some((entry) => isModTab(entry) && editorTabKey(entry) === key),
    ),
  );
  const target = existing ?? tabs.find((tab) => tab.id === targetTabId);
  if (!target) return { tabs, tabId: null };
  return {
    tabs: tabs.map((tab) =>
      tab === target ? openEditorTab(tab, file) : tab,
    ),
    tabId: target.id,
  };
}
