import type { OrchestrationTone } from "@/features/workspace/types";
import { useColors, useScheme } from "@/ui/theme";

/** The composer's Orchestrator mode tint, which marks a lead's run everywhere. */
const ACCENT = { dark: "#f0abfc", light: "#a21caf" } as const;
/** The desktop card's sky for work in progress, darker on light. */
const WORKING = { dark: "#7dd3fc", light: "#0369a1" } as const;

/** Each assignment tone's colour, as the desktop card draws it. */
export function useToneColors() {
  const colors = useColors();
  const scheme = useScheme();
  const tone = (value: OrchestrationTone) => {
    switch (value) {
      case "starting":
      case "working":
        return WORKING[scheme];
      case "done":
        return colors.additions;
      case "failed":
        return colors.deletions;
      case "input":
      case "blocked":
      case "interrupted":
        return colors.caution;
      default:
        return colors.muted;
    }
  };
  return { accent: ACCENT[scheme], tone };
}
