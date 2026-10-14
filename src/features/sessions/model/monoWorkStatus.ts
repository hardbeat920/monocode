import { t } from "../../../shared/i18n/locale";
import { isReadTool } from "../../../integrations/harness/core/preview";
import type { Block } from "./session";
import {
  isProseBlock,
  isThinkingBlock,
  isToolBlock,
  needsApproval,
  subagentFailureSummary,
  toolCategory,
  workKind,
  workSummaryLine,
  type ActivityPhaseKind,
} from "./transcriptActivity";

function thinkingLabels(): string[] {
  return [
    t("sessions.monoStatus.thinking1"),
    t("sessions.monoStatus.thinking2"),
    t("sessions.monoStatus.thinking3"),
    t("sessions.monoStatus.thinking4"),
  ];
}

export type MonoWorkStatus = {
  key: string;
  label: string;
  kind: ActivityPhaseKind;
  active: boolean;
};

/** A short status for the latest activity, stable across its streamed chunks. */
export function monoWorkStatus(
  blocks: Block[],
  active: boolean,
): MonoWorkStatus {
  if (!active) {
    return {
      key: "summary",
      label: subagentFailureSummary(blocks) ?? workSummaryLine(blocks),
      kind: workKind(blocks),
      active: false,
    };
  }
  const waiting = blocks.find(needsApproval);
  if (waiting) {
    return {
      key: `approval:${waiting.id}`,
      label: t("sessions.monoStatus.waitingApproval"),
      kind: toolCategory(waiting),
      active: true,
    };
  }
  let thought = -1;
  let latest: Block | undefined;
  for (const block of blocks) {
    if (isProseBlock(block) || isThinkingBlock(block)) {
      thought += 1;
      latest = block;
    } else if (isToolBlock(block)) {
      latest = block;
    }
  }
  if (!latest || !isToolBlock(latest)) {
    const labels = thinkingLabels();
    return {
      key: latest?.id ?? "thinking",
      label: labels[Math.max(0, thought) % labels.length],
      kind: "think",
      active: true,
    };
  }
  const kind = toolCategory(latest);
  const label = {
    run: t("sessions.monoStatus.runCommand"),
    edit: t("sessions.monoStatus.editFile"),
    research: isReadTool(
      latest.tool?.kind,
      latest.text || latest.tool?.title,
      latest.tool?.preview,
    )
      ? t("sessions.monoStatus.readFile")
      : t("sessions.monoStatus.exploreProject"),
    agent: t("sessions.monoStatus.runAgent"),
    other: t("sessions.monoStatus.useTool"),
  }[kind];
  return { key: latest.id, label, kind, active: true };
}
