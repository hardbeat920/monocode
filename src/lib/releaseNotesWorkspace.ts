import { isReleaseNotesTab, type WorkspaceTab } from "./layout";

export type ReleaseNotesOpenPlan =
  | { kind: "open" }
  | {
      kind: "focus";
      tabId: string;
      paneId: string;
      fileId: string;
    };

type ReleaseNotesFocusTarget = Extract<ReleaseNotesOpenPlan, { kind: "focus" }>;

export function planReleaseNotesOpen(
  tabs: WorkspaceTab[],
  version: string,
): ReleaseNotesOpenPlan {
  for (const tab of tabs) {
    for (const pane of tab.editorPanes) {
      const file = pane.files.find(
        (entry) =>
          isReleaseNotesTab(entry) && entry.releaseNotes.version === version,
      );
      if (file) {
        return {
          kind: "focus",
          tabId: tab.id,
          paneId: pane.id,
          fileId: file.id,
        };
      }
    }
  }
  return { kind: "open" };
}

export function focusReleaseNotesTarget(
  tabs: WorkspaceTab[],
  target: ReleaseNotesFocusTarget,
): WorkspaceTab[] {
  return tabs.map((tab) =>
    tab.id === target.tabId
      ? {
          ...tab,
          focusedId: target.paneId,
          diffFocused: false,
          editorPanes: tab.editorPanes.map((pane) =>
            pane.id === target.paneId
              ? { ...pane, activeFileId: target.fileId }
              : pane,
          ),
        }
      : tab,
  );
}
