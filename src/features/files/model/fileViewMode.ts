import type { MarkdownViewMode } from "../../sessions/ui/MarkdownModeToggle";

const KEY = "monocode.fileViewMode";

// Holds a choice only while storage failed to keep it, so later files in this
// window still open in that view when localStorage is unavailable.
let unsaved: MarkdownViewMode | null = null;

/** The view a previewable file opens in until that file gets its own choice. */
export function loadFileViewMode(): MarkdownViewMode {
  if (unsaved) return unsaved;
  try {
    return localStorage.getItem(KEY) === "source" ? "source" : "preview";
  } catch {
    return "preview";
  }
}

export function saveFileViewMode(mode: MarkdownViewMode): void {
  try {
    localStorage.setItem(KEY, mode);
    unsaved = null;
  } catch {
    unsaved = mode;
  }
}
