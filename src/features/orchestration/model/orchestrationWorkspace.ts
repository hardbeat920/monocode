import { closeLeaf, leafIds, type WorkspaceTab } from "../../workspace/model/layout";
import type { OrchestrationRun } from "./orchestration";
import type { Session } from "../../sessions/model/session";

export function releaseOrchestrationWorker(
  session: Session,
  leadId: string,
): Session {
  if (
    session.orchestrationLeadId !== leadId &&
    !session.blocks.some((block) => block.orchestrationLeadId === leadId)
  )
    return session;
  return {
    ...session,
    orchestrationLeadId:
      session.orchestrationLeadId === leadId
        ? undefined
        : session.orchestrationLeadId,
    blocks: session.blocks.map((block) =>
      block.orchestrationLeadId === leadId
        ? { ...block, orchestrationLeadId: undefined }
        : block,
    ),
  };
}

/** Load the lead first so a fast worker read cannot publish panes without a tab. */
export async function prepareOrchestrationWorkerDetails<
  T extends { leadId: string; sessionId: string },
>(
  workers: T[],
  host: {
    openLead: (id: string) => Promise<void>;
    openWorker: (id: string) => Promise<unknown>;
    hasSession: (id: string) => boolean;
  },
) {
  const list = workers.filter(
    (worker) => worker.leadId && worker.leadId !== worker.sessionId,
  );
  if (!list.length) return null;
  const leadId = list[0].leadId;
  await host.openLead(leadId);
  if (!host.hasSession(leadId)) return null;
  await Promise.all(list.map((worker) => host.openWorker(worker.sessionId)));
  if (!host.hasSession(leadId)) return null;
  return {
    leadId,
    workers: list.filter((worker) => host.hasSession(worker.sessionId)),
  };
}

/**
 * Adopt worker tabs made by the earlier preview into an already-open lead.
 * A worker the user asked to inspect is a tab inside an editor pane rather
 * than a session leaf, so it never reaches this.
 */
export function consolidateOrchestrationTabs(
  tabs: WorkspaceTab[],
  activeTabId: string,
  runs: OrchestrationRun[],
) {
  const parents = new Map(
    runs.flatMap((run) =>
      tabs.some((tab) => leafIds(tab.layout).includes(run.leadId))
        ? run.tasks.map((task) => [task.sessionId, run.leadId] as const)
        : [],
    ),
  );
  const active = tabs.find((tab) => tab.id === activeTabId);
  const lead = active && parents.get(active.focusedId);
  let changed = false;
  const next = tabs.flatMap((tab) => {
    let remaining: WorkspaceTab | null = tab;
    for (const id of leafIds(tab.layout)) {
      if (remaining && parents.has(id)) {
        remaining = closeLeaf(remaining, id);
        changed = true;
      }
    }
    return remaining ? [remaining] : [];
  });
  return {
    tabs: changed ? next : tabs,
    activeTabId: lead
      ? next.find((tab) => leafIds(tab.layout).includes(lead))!.id
      : activeTabId,
  };
}

export function attachOrchestrationWorkers(
  sessions: Session[],
  runs: OrchestrationRun[],
) {
  const parents = new Map(
    runs.flatMap((run) =>
      run.tasks.map((task) => [task.sessionId, run.leadId] as const),
    ),
  );
  let changed = false;
  const next = sessions.map((session) => {
    const parent = parents.get(session.id);
    if (!parent || session.orchestrationLeadId === parent) return session;
    changed = true;
    return { ...session, orchestrationLeadId: parent };
  });
  return changed ? next : sessions;
}
