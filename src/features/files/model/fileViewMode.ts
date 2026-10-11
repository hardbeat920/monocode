import type { MarkdownViewMode } from "../../sessions/ui/MarkdownModeToggle";

const KEY = "monocode.fileViewMode";

/** The view a previewable file opens in until that file gets its own choice. */
export function loadFileViewMode(): MarkdownViewMode {
  try {
    return localStorage.getItem(KEY) === "source" ? "source" : "preview";
  } catch {
    return "preview";
  }
}

export function saveFileViewMode(mode: MarkdownViewMode): void {
  try {
    localStorage.setItem(KEY, mode);
  } catch {
    // private mode / quota
  }
}
