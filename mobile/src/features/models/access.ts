import type { IconName } from "@/ui/Icon";

import type { RuntimeMode } from "./types";

/**
 * The desktop's permission modes (RUNTIME_MODES with their labels and hints
 * in src/features/sessions/model/session.ts), with the AccessPicker's icons.
 */
export const ACCESS_MODES: {
  id: RuntimeMode;
  label: string;
  hint: string;
  icon: IconName;
}[] = [
  {
    id: "supervised",
    label: "Supervised",
    hint: "Ask before commands and file changes.",
    icon: "lock",
  },
  {
    id: "auto-accept-edits",
    label: "Auto-accept edits",
    hint: "Auto-approve edits, ask before other actions.",
    icon: "edit",
  },
  {
    id: "auto",
    label: "Auto",
    hint: "An AI reviewer can approve or deny actions.",
    icon: "sparkle",
  },
  {
    id: "full-access",
    label: "Full access",
    hint: "Allow commands, edits, and supported MCP confirmations in non-plan turns without prompts.",
    icon: "shield",
  },
];

export const DEFAULT_ACCESS: RuntimeMode = "supervised";

export function accessMode(id: RuntimeMode | undefined) {
  return ACCESS_MODES.find((mode) => mode.id === id) ?? ACCESS_MODES[0];
}
