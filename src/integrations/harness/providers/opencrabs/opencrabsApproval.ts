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
    target.approvals.set(id, resolve);
  });
  target.approvals.delete(id);
  target.onEvent({ type: "approval.resolved", requestId: id, decision });

  await target.acp.respond(id, {
    outcome: {
      outcome: "selected",
      optionId: permissionOptionId(decision, request.optionIds),
    },
  });
}
