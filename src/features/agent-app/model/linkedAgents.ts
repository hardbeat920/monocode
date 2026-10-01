import type { AppSessionListing } from "./agentApp";
import type { SessionLinkView } from "./sessionLinks";
import { assignmentTask } from "./assignments";
import {
  HARNESSES,
  type HarnessId,
  type Block,
  type Session,
} from "../../sessions/model/session";
import { pendingApprovalForSession } from "../../notifications/model/approvalToast";
import { composeToolTitle } from "../../../integrations/harness/core/preview";

export type LinkedAgentListing = AppSessionListing & {
  name?: string;
  taskPreview?: string;
  taskTruncated?: boolean;
  state: string;
  activity: string;
  observed: boolean;
  removed: boolean;
};

/** Both the callable list and bound-agent UI use caller-owned links, never project-wide inference. */
export function linkedAgentListings(
  links: SessionLinkView[],
  listings: AppSessionListing[],
  parentBlocks: Block[],
  openSession: (id: string) => Session | undefined,
): LinkedAgentListing[] {
  const byId = new Map(listings.map((item) => [item.id, item]));
  return links.map((link) => {
    const live = link.removing ? undefined : openSession(link.childId);
    const listed = byId.get(link.childId);
    const task =
      link.assignment && assignmentTask(parentBlocks, link.assignment);
    const waiting = live && pendingApprovalForSession(live);
    let activity = live ? (live.busy ? "Working" : "Idle") : "Unobserved";
    if (live?.busy) {
      for (let index = live.blocks.length - 1; index >= 0; index--) {
        const block = live.blocks[index];
        if (block.role === "user") break;
        if (block.tool || block.role === "approval") {
          activity =
            composeToolTitle({
              kind: block.tool?.kind,
              title: block.text || block.tool?.title,
              cwd: live.cwd,
            }) || "Working";
          break;
        }
      }
    }
    const { childId: _childId, ...tracked } = link;
    return {
      id: link.childId,
      title: live?.title ?? listed?.title ?? link.title ?? link.childId,
      harness:
        live?.harness ??
        listed?.harness ??
        (HARNESSES.includes(link.harness as HarnessId)
          ? (link.harness as HarnessId)
          : "unknown"),
      model: live?.model ?? listed?.model ?? link.model ?? "unknown",
      busy: !!live?.busy,
      hasDraft: listed?.hasDraft ?? false,
      tracked,
      ...(link.assignment?.name ? { name: link.assignment.name } : {}),
      ...(task !== undefined
        ? { taskPreview: task.slice(0, 240), taskTruncated: task.length > 240 }
        : {}),
      state: link.removing
        ? "removed"
        : (waiting?.kind ?? (link.held ? "held" : link.status)),
      activity: waiting?.label ?? activity,
      observed: !!live,
      removed: !!link.removing,
    };
  });
}
