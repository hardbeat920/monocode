import type { Session } from "../session";
import { isTaskListToolName } from "../taskList";
import { applyHarnessEvent } from "./apply";
import {
  readStoredCursorSubagentRuns,
  type StoredCursorSubagentRun,
} from "./cursorStore";
import {
  extractToolPreview,
  formatAgentType,
  titleFromToolInput,
} from "./preview";
import type { HarnessEvent } from "./types";

export function cursorAgentLabel(value?: string | null): string | undefined {
  const label = value
    ?.trim()
    .replace(/^[:\s·-]+/, "")
    .replace(/^(?:agent|task)\s*[:·-]\s*/i, "")
    .trim();
  return label &&
    !/^(?:subagent(?:\s+task)?|task|agent|unspecified)$/i.test(label)
    ? label
    : undefined;
}

function promptLabel(prompt?: string | null): string | undefined {
  const line = prompt?.trim().split("\n")[0]?.trim();
  if (!line) return undefined;
  return line
    .replace(
      /^(?:perform|conduct|do)\b.*?\b(?:code\s+)?review\s+of\s+/i,
      "Review ",
    )
    .replace(/\s+in\s+(?:\/|[A-Za-z]:[\\/]).*$/, "")
    .replace(/[.\s]+$/, "");
}

export function cursorSubagentEvents(
  run: StoredCursorSubagentRun,
  title?: string,
): HarnessEvent[] {
  const typeLabel = run.agentType
    ? `${formatAgentType(run.agentType)} subagent`
    : "Subagent";
  const label = cursorAgentLabel(title);
  const name =
    (label === typeLabel ? undefined : label) ??
    promptLabel(run.prompt) ??
    typeLabel;
  const events: HarnessEvent[] = [
    {
      type: "tool.updated",
      callId: run.toolCallId,
      kind: "agent",
      title: name,
      ...(run.model ? { agentModel: run.model } : {}),
    },
  ];
  for (const step of run.steps) {
    const args =
      step.args && typeof step.args === "object" && !Array.isArray(step.args)
        ? (step.args as Record<string, unknown>)
        : {};
    const kind = kindFromCursorToolName(step.toolName);
    const toolName = step.toolName ?? "Tool";
    const tool = { name: toolName, kind, rawInput: args, content: step.output };
    const preview = extractToolPreview(tool, tool);
    events.push({
      type: "agent.step",
      callId: run.toolCallId,
      stepId: step.id,
      kind: step.kind,
      agentName: name,
      ...(run.agentType ? { agentType: run.agentType } : {}),
      text:
        step.kind === "tool"
          ? titleFromToolInput(toolName, kind ?? "other", args)
          : step.text,
      ...(step.kind === "tool"
        ? {
            toolKind: kind,
            status: step.status,
            preview: step.output
              ? {
                  ...(preview ?? { kind: "read" as const, contentOnly: true }),
                  output: step.output,
                }
              : preview,
          }
        : {}),
    });
  }
  return events;
}

/** Reopen older Cursor rows with the work already saved in their child stores. */
export async function recoverCursorSubagents(
  session: Session,
): Promise<Session> {
  if (session.harness !== "cursor" || !session.providerSessionId)
    return session;
  const rows = session.blocks.filter(
    (block) => block.tool?.kind === "agent" && block.tool.callId,
  );
  if (!rows.length) return session;
  const runs = await readStoredCursorSubagentRuns(
    session.providerSessionId,
    rows.map((row) => row.tool!.callId!).slice(-256),
  ).catch(() => []);
  const titles = new Map(
    rows.map((row) => [row.tool!.callId, row.tool!.title ?? row.text]),
  );
  for (const run of runs) {
    if (!titles.has(run.toolCallId)) continue;
    for (const event of cursorSubagentEvents(run, titles.get(run.toolCallId)))
      session = applyHarnessEvent(session, event);
  }
  return session;
}

export function kindFromCursorToolName(
  name: string | undefined,
  fallback?: string,
): string | undefined {
  const key = (name ?? "").toLowerCase();
  if (
    key === "grep" ||
    key === "glob" ||
    key === "rg" ||
    key.includes("search")
  )
    return "search";
  if (key === "read") return "read";
  if (["edit", "write", "strreplace", "applypatch"].includes(key))
    return "edit";
  if (key === "shell" || key === "bash") return "execute";
  if (key === "skill" || key === "skills") return "skill";
  if (key === "agent" || key === "task" || key === "subagent") return "agent";
  if (isTaskListToolName(key)) return "tasks";
  return fallback;
}
