import { relayedApproval } from "../../monos/model/monoHabits";
import type { ApprovalDecision } from "../../../integrations/harness";
import type { Block, Session } from "../../sessions/model/session";
import { isRemoteProjectPath } from "../../projects/model/recents";
import { toolCallLabel } from "../../sessions/model/transcriptActivity";

export type MobileApproval = {
  token: string;
  requestId: number;
  title: string;
  detail: string;
  decisions: { id: ApprovalDecision; label: string }[];
  unsupported?: string;
};
// Approval objects survive tool output updates, but every new request creates
// a new object, even when both its block and provider request ID are reused.
const resolved = new WeakSet<NonNullable<Block["approval"]>>();
export function markMobileApprovalResolved(
  session: Session,
  requestId: number,
) {
  for (const block of session.blocks)
    if (block.approval?.requestId === requestId) resolved.add(block.approval);
}
const tokens = new WeakMap<NonNullable<Block["approval"]>, string>();
function tokenFor(approval: NonNullable<Block["approval"]>): string {
  let token = tokens.get(approval);
  if (!token) tokens.set(approval, (token = crypto.randomUUID()));
  return token;
}
export function mobileApprovals(
  session: Session,
  live: readonly Session[] = [session],
): MobileApproval[] {
  // Keep approval metadata well below the link's 8 MiB response limit, even
  // after JSON escaping. Incomplete context must be reviewed on desktop.
  let remaining = 64_000;
  const pending = session.blocks.filter((block) => block.approval &&
    !block.approval.decided && !resolved.has(block.approval));
  const approvals = pending.slice(0, 64).map((block): MobileApproval => {
    const approval = block.approval!;
    const preview = block.tool?.preview;
    const relay = block.monoHabit
      ? relayedApproval(session.id, approval.requestId)
      : undefined;
    const run = relay ? live.find((s) => s.id === relay.runId) : undefined;
    const relayAvailable =
      !block.monoHabit ||
      (run &&
        !run.worktreeRemoved &&
        !isRemoteProjectPath(run.cwd) &&
        run.blocks.some(
          (b) =>
            b.approval != null &&
            b.approval === relay?.approval &&
            b.approval.requestId === relay.requestId &&
            !b.approval.decided,
        ));
    const title = toolCallLabel(block, session.cwd);
    const detail = [
      preview?.path, preview?.title, preview?.query,
      preview?.lines?.map((line) =>
        `${line.kind === "add" ? "+" : line.kind === "del" ? "-" : " "}${line.text}`,
      ).join("\n"),
      preview?.output, block.tool?.detail,
    ].filter(Boolean).join("\n");
    const limit = Math.min(16_000, remaining);
    remaining -= Math.min(detail.length, limit);
    const truncated = detail.length > limit || title.length > 512;
    const unsupported = truncated
      ? "Context is too large to show completely. Review and respond on the desktop."
      : !relayAvailable
        ? "This Mono approval's run is no longer available. Resolve this on the desktop."
        : session.worktreeRemoved
          ? "This session's worktree was removed. Resolve this on the desktop."
          : isRemoteProjectPath(session.cwd)
            ? "Respond on the desktop: this remote connection cannot confirm mobile approval delivery."
            : undefined;
    return {
      token: tokenFor(approval),
      requestId: approval.requestId,
      title: title.slice(0, 512),
      detail: detail.slice(0, limit),
      decisions: unsupported
        ? []
        : [
            { id: "deny" as const, label: "Deny" },
            { id: "allow" as const, label: "Allow once" },
          ],
      ...(unsupported ? { unsupported } : {}),
    };
  });
  if (pending.length > 64) approvals.push({
    token: "overflow", requestId: -1, title: "More approvals are waiting", detail: "",
    decisions: [], unsupported: "Review the remaining approvals on the desktop.",
  });
  return approvals;
}
/** Revalidate immediately before delivery, against the live session. */
export function mobileApprovalDecision(
  session: Session,
  params: Record<string, unknown>,
  live?: readonly Session[],
): MobileApproval {
  const approval = mobileApprovals(session, live).find(
    (item) =>
      item.token === params.token && item.requestId === params.requestId,
  );
  if (!approval)
    throw new Error("This approval was already resolved or replaced");
  if (approval.unsupported) throw new Error(approval.unsupported);
  if (!approval.decisions.some((decision) => decision.id === params.decision))
    throw new Error("This decision is not available for this approval");
  return approval;
}

/** The command handler used after ordinary-session or Mono routing resolves. */
export function respondMobileApproval(
  sessionId: string,
  params: Record<string, unknown>,
  live: readonly Session[],
  respond: (id: string, requestId: number, decision: ApprovalDecision) => void,
) {
  const session = live.find((current) => current.id === sessionId);
  if (!session) throw new Error("Session is no longer available");
  const approval = mobileApprovalDecision(session, params, live);
  respond(session.id, approval.requestId, params.decision as ApprovalDecision);
  return { sessionId: session.id };
}
