// Copied from TAB_GROUP_COLORS and tabGroupColor in the desktop app's
// src/features/workspace/model/tabGroups.ts. Keep the two in sync.
export const PROJECT_COLORS = [
  "hsl(210 8% 58%)",
  "hsl(211 92% 62%)",
  "hsl(12 80% 58%)",
  "hsl(45 90% 55%)",
  "hsl(142 55% 50%)",
  "hsl(330 70% 62%)",
  "hsl(280 55% 62%)",
  "hsl(175 55% 48%)",
  "hsl(25 85% 58%)",
] as const;

/** Stable per-project color; skips the neutral first entry like desktop. */
export function projectColor(project: string): string {
  let hash = 0;
  for (let i = 0; i < project.length; i++) {
    hash = (hash * 31 + project.charCodeAt(i)) >>> 0;
  }
  return PROJECT_COLORS[(hash % (PROJECT_COLORS.length - 1)) + 1];
}
