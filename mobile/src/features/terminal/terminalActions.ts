import { useCallback, useState } from "react";

import { useConnection } from "@/features/connection/ConnectionProvider";
import { request } from "@/features/connection/nativeLink";
import type { ProjectAction } from "@/features/workspace/types";
import { useWorkspace } from "@/features/workspace/WorkspaceProvider";

/** An action as the phone adds it: the desktop gives it its ID. */
export type NewAction = Omit<ProjectAction, "id"> & { id?: string };

/** The SF Symbol for each of the desktop's action icons. */
export const ACTION_SYMBOLS: Record<string, string> = {
  play: "play",
  "test-tube": "testtube.2",
  hammer: "hammer",
  rocket: "paperplane",
  bug: "ant",
  package: "shippingbox",
};

/**
 * A project's actions: the desktop footer's, the same list on both, and how
 * to change them. A change is shown at once and sent as the whole list; the
 * next workspace refresh brings the desktop's copy, IDs and all.
 */
export function useTerminalActions(projectId: string) {
  const { pairing, refresh } = useConnection();
  const project = useWorkspace().projects.find((item) => item.id === projectId);
  const [sending, setSending] = useState<NewAction[] | null>(null);
  const actions: NewAction[] = sending ?? project?.actions ?? [];

  const save = useCallback(
    async (next: NewAction[]) => {
      if (!pairing) return;
      setSending(next);
      try {
        await request(pairing, "terminal.actions.save", { projectId, actions: next });
        await refresh();
      } finally {
        setSending(null);
      }
    },
    [pairing, projectId, refresh],
  );
  return [actions, save] as const;
}
