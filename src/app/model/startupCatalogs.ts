import type { Session } from "../../features/sessions/model/session";
import { refreshHarnessCatalogs } from "../../integrations/harness/core/registry";
import { refreshProjectOpenCodeCatalog } from "../../integrations/harness/providers/opencode/opencodeCatalog";

/** Refresh the catalogs that the window's restored sessions use. OpenCode
 * refreshes its home catalog too, because orchestration worker choices read
 * `modelsFor("opencode")`, which ignores the per-directory lists. */
export async function refreshStartupCatalogs(
  sessions: readonly Pick<Session, "harness" | "cwd" | "worktreeCwd">[],
): Promise<void> {
  const harnesses = [...new Set(sessions.map((session) => session.harness))];
  const openCodeDirectories = [
    ...new Set(
      sessions
        .filter((session) => session.harness === "opencode")
        .map((session) => session.worktreeCwd ?? session.cwd),
    ),
  ];
  await Promise.all([
    refreshHarnessCatalogs(harnesses),
    ...openCodeDirectories.map(refreshProjectOpenCodeCatalog),
  ]);
}
