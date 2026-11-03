import type { Session } from "../../sessions/model/session";
import {
  isRemoteProjectPath,
  sameProjectPath,
} from "../../projects/model/recents";
import { leafIds, type WorkspaceTab } from "../../workspace/model/layout";
import { remoteSessionFor } from "./connections";

/** Only reuse a remote shell that belongs to the requested project. */
export function findRemoteSessionTab(
  tabs: readonly WorkspaceTab[],
  sessions: readonly Pick<Session, "id" | "cwd">[],
  project: string,
  sessionId: string,
): { tab: WorkspaceTab; shellId: string } | undefined {
  if (!isRemoteProjectPath(project)) return undefined;
  const cwdBySession = new Map(
    sessions.map((session) => [session.id, session.cwd]),
  );
  for (const tab of tabs) {
    for (const shellId of leafIds(tab.layout)) {
      const cwd = cwdBySession.get(shellId);
      if (
        cwd &&
        sameProjectPath(cwd, project) &&
        remoteSessionFor(shellId) === sessionId
      ) {
        return { tab, shellId };
      }
    }
  }
}
