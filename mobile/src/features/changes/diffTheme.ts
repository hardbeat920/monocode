import { Platform } from "react-native";

import type { ChangeStatus } from "@/features/workspace/types";
import { useScheme } from "@/ui/theme";

export const MONO = Platform.select({ ios: "Menlo", default: "monospace" });

// Status letters from the desktop GitChangesPanel.
export const STATUS_LETTER: Record<ChangeStatus, string> = {
  modified: "M",
  added: "A",
  deleted: "D",
  untracked: "U",
};

// Status and diff colors from the desktop GitChangesPanel and UnifiedDiffView
// (diff-add / diff-del tokens), with its light theme's darker text.
const ROWS = {
  add: { row: "rgba(16, 185, 129, 0.15)", gutter: "rgba(16, 185, 129, 0.25)" },
  del: { row: "rgba(244, 63, 94, 0.15)", gutter: "rgba(244, 63, 94, 0.25)" },
};
const DIFF_THEMES = {
  dark: {
    status: { modified: "#fbbf24", added: "#6ee7b7", deleted: "#fda4af", untracked: "#38bdf8" },
    add: { ...ROWS.add, fg: "#6ee7b7" },
    del: { ...ROWS.del, fg: "#fda4af" },
  },
  light: {
    status: { modified: "#b45309", added: "#047857", deleted: "#be123c", untracked: "#0284c7" },
    add: { ...ROWS.add, fg: "#047857" },
    del: { ...ROWS.del, fg: "#be123c" },
  },
} satisfies Record<string, { status: Record<ChangeStatus, string>; add: object; del: object }>;

/** Status and diff colors in the scheme the app is drawn in. */
export function useDiffTheme() {
  return DIFF_THEMES[useScheme()];
}

/** Splits a relative path into its file name and containing folder. */
export function splitPath(relative: string) {
  const slash = relative.lastIndexOf("/");
  return { name: relative.slice(slash + 1), dir: slash > 0 ? relative.slice(0, slash) : "" };
}

/** A count with thousands separated, as 18,300 rather than 18300. */
export function formatCount(n: number) {
  return String(n).replace(/\B(?=(\d{3})+(?!\d))/g, ",");
}
