import type { ApprovalDecision, HarnessEvent } from "../../core/types";
import type { AcpClient } from "../../core/acp";
import type { RuntimeMode } from "../../../../features/sessions/model/session";
import {
  autoPermissionOption,
  permissionOptionId,
  permissionRequestFromAcp,
} from "./opencrabsProtocol";

export type ApprovalTarget = {
  acp: AcpClient;
  planning: boolean;
  runtimeMode: RuntimeMode;
  onEvent: (event: HarnessEvent) => void;
  approvals: Map<number, (decision: ApprovalDecision) => void>;
};

/** The server abandons an unanswered permission after 300s and proceeds as
 * denied (PERMISSION_TIMEOUT in its turn bridge). The dialog must not
 * outlive that deadline — answering after the server moved on sends a
 * response nobody waits for, and an eternally open dialog lies about the
 * session's state. Expire just under the server deadline, resolving deny,
 * so UI and server stay in agreement. */
const APPROVAL_EXPIRY_MS = 290_000;

export async function handlePermission(
  target: ApprovalTarget,
  id: number,
  params: unknown,
): Promise<void> {
  const request = permissionRequestFromAcp(params);
  if (request.callId) {
    target.onEvent({
      type: "tool.updated",
      callId: request.callId,
      title: request.title,
      kind: request.kind,
      preview: request.preview,
    });
  }

  if (target.planning) {
    const normalized = (request.kind ?? "").toLowerCase();
    const readOnly = normalized === "read" || normalized === "search";
    await target.acp.respond(id, {
      outcome: {
        outcome: "selected",
        optionId: permissionOptionId(readOnly ? "allow" : "deny", request.optionIds),
      },
    });
    return;
  }

  const auto = autoPermissionOption(
    target.runtimeMode,
    request.kind,
    request.optionIds,
  );
  if (auto) {
    await target.acp.respond(id, {
      outcome: { outcome: "selected", optionId: auto },
    });
    return;
  }

  target.onEvent({
    type: "approval.requested",
    requestId: id,
    title: request.title,
    kind: request.kind,
    callId: request.callId,
    preview: request.preview,
  });

  const decision = await new Promise<ApprovalDecision>((resolve) => {
    // Settle exactly once: user answer, cancel sweep, or expiry — whichever
    // comes first clears the timer and the map slot, so a late timer or a
    // late click can never double-resolve.
    const settle = (outcome: ApprovalDecision) => {
      clearTimeout(timer);
      target.approvals.delete(id);
      resolve(outcome);
    };
    const timer = setTimeout(() => {
      settle("deny");
    }, APPROVAL_EXPIRY_MS);
    target.approvals.set(id, settle);
  });
  target.onEvent({ type: "approval.resolved", requestId: id, decision });

  await target.acp.respond(id, {
    outcome: {
      outcome: "selected",
      optionId: permissionOptionId(decision, request.optionIds),
    },
  });
}
