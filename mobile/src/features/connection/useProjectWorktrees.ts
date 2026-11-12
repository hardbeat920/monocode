import { useEffect, useState } from "react";
import type { Worktree } from "@/features/workspace/types";
import { useConnection } from "./ConnectionProvider";
/** The last list read for each project, shown at once while it is read again. */
const lastRead = new Map<string, Worktree[]>();
/**
 * The project's other worktrees, fetched once per screen for the workspace
 * menu. `loaded` once there is a list to go by, from this read or the last.
 * Read only once `enabled`; until then the last list stands.
 */
export function useProjectWorktrees(projectId: string, enabled = true) {
  const { command, pairing, connected } = useConnection();
  const scope = `${pairing?.desktop.fingerprint ?? ""}\n${projectId}\n${connected}\n${enabled}`;
  const [capability, setCapability] = useState({ scope: "", cowSupported: false });
  const [worktrees, setWorktrees] = useState(() => lastRead.get(projectId));
  if (capability.scope !== scope) setCapability({ scope, cowSupported: false });
  useEffect(() => {
    if (!enabled || !connected) return;
    let disposed = false;
    command<{ worktrees: Worktree[]; cowSupported?: boolean }>("project.worktrees", { projectId })
      .then((value) => {
        lastRead.set(projectId, value.worktrees);
        if (!disposed) {
          setWorktrees(value.worktrees);
          setCapability({ scope, cowSupported: value.cowSupported === true });
        }
      })
      // An older desktop doesn't answer this; the menu then offers no existing ones.
      .catch(() => {
        if (!disposed) setWorktrees((current) => current ?? []);
      });
    return () => {
      disposed = true;
    };
  }, [projectId, command, enabled, connected, scope]);
  return {
    worktrees: worktrees ?? [],
    loaded: worktrees !== undefined,
    cowSupported: connected && capability.scope === scope && capability.cowSupported,
  };
}
