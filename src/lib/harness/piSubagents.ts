import type { HarnessEvent } from "./types";
import {
  asRecord,
  previewFromTool,
  stringField,
  textFromContent,
  toolKindFromName,
  toolTitle,
} from "./piProtocol";

/** Pi extension results and omp TaskToolDetails are cumulative snapshots. */
export function piSubagentEvents(
  callId: string,
  input: Record<string, unknown>,
  result: unknown,
  completed: boolean,
  isError = false,
): HarnessEvent[] {
  const details = asRecord(asRecord(result)?.details);
  if (!details) return [];
  const results = records(details.results);
  const progress = records(details.progress);
  const entries = progress.length ? progress : results;
  if (!entries.length) return [];
  const batch =
    entries.length > 1 ||
    Array.isArray(input.tasks) ||
    Array.isArray(input.chain);
  const events: HarnessEvent[] = [];
  if (batch) {
    events.push({
      type: "tool.updated",
      callId,
      kind: "other",
      title: "Delegate subagents",
    });
  }
  entries.forEach((entry, index) => {
    const key = entry.index ?? index;
    const rowId = batch ? `${callId}:agent:${key}` : callId;
    const final = progress.length
      ? (results.find(
          (item) =>
            (item.id === entry.id && item.id != null) || item.index === key,
        ) ?? {})
      : entry;
    const failed =
      entry.status === "failed" ||
      entry.status === "aborted" ||
      final.aborted === true ||
      final.stopReason === "error" ||
      final.stopReason === "aborted" ||
      (typeof final.exitCode === "number" && final.exitCode > 0) ||
      (completed && isError && final.exitCode == null && entry.status == null);
    const background = asRecord(details.async)?.state === "running";
    const status = failed
      ? "failed"
      : entry.status === "completed" || (completed && !background)
        ? "completed"
        : "in_progress";
    const name =
      stringField(entry, "description") ??
      stringField(entry, "task") ??
      stringField(entry, "agent") ??
      "Subagent";
    const agentType = stringField(entry, "agent");
    const report =
      stringField(final, "output") ??
      stringField(final, "errorMessage") ??
      stringField(final, "error") ??
      (failed
        ? (stringField(final, "stderr") ?? "Subagent failed.")
        : undefined);
    events.push({
      type: "tool.updated",
      callId: rowId,
      kind: "agent",
      title: name,
      status,
      ...(report ? { detail: report } : {}),
    });
    const emit = (
      step: Omit<
        Extract<HarnessEvent, { type: "agent.step" }>,
        "type" | "callId" | "agentName" | "agentType"
      >,
    ) => {
      events.push({
        type: "agent.step",
        callId: rowId,
        agentName: name,
        agentType,
        ...step,
      });
    };
    const messages = records(final.messages);
    const toolResults = new Map(
      records(final.messages)
        .filter((message) => message.role === "toolResult")
        .map((message) => [message.toolCallId, message]),
    );
    messages.forEach((message, messageIndex) => {
      if (message.role !== "assistant") return;
      records(message.content).forEach((part, partIndex) => {
        const stepId = `${rowId}:message:${messageIndex}:${partIndex}`;
        if (part.type === "text" || part.type === "thinking") {
          const text = stringField(
            part,
            part.type === "text" ? "text" : "thinking",
          );
          if (text)
            emit({
              stepId,
              kind: part.type === "text" ? "message" : "reasoning",
              text,
            });
        } else if (part.type === "toolCall") {
          const id = stringField(part, "id");
          const tool = stringField(part, "name") ?? "tool";
          const args = asRecord(part.arguments) ?? {};
          const outcome = toolResults.get(id);
          emit({
            stepId: `${rowId}:tool:${id ?? `${messageIndex}:${partIndex}`}`,
            kind: "tool",
            text: toolTitle(tool, args),
            toolKind: toolKindFromName(tool),
            status: outcome
              ? outcome.isError
                ? "failed"
                : "completed"
              : status === "in_progress"
                ? "in_progress"
                : status,
            preview: previewFromTool(
              tool,
              args,
              textFromContent(outcome?.content),
            ),
          });
        }
      });
    });
    // omp reports newest-first bounded tails and an increasing toolCount.
    const count = typeof entry.toolCount === "number" ? entry.toolCount : 0;
    const current = stringField(entry, "currentTool");
    records(entry.recentTools)
      .slice()
      .reverse()
      .forEach((tool, index, all) => {
        const name = stringField(tool, "tool");
        if (!name) return;
        const number = count - (current ? 1 : 0) - all.length + index + 1;
        emit({
          stepId: `${rowId}:tool:${number}`,
          kind: "tool",
          text: [name, stringField(tool, "args")].filter(Boolean).join(" "),
          toolKind: toolKindFromName(name),
          status: "completed",
        });
      });
    if (current)
      emit({
        stepId: `${rowId}:tool:${count}`,
        kind: "tool",
        text: [current, stringField(entry, "currentToolArgs")]
          .filter(Boolean)
          .join(" "),
        toolKind: toolKindFromName(current),
        status,
      });
    const output = Array.isArray(entry.recentOutput)
      ? entry.recentOutput
          .filter((line): line is string => typeof line === "string")
          .slice()
          .reverse()
          .join("\n")
      : "";
    if (output)
      emit({
        stepId: `${rowId}:output:${entry.requests ?? count}`,
        kind: "message",
        text: output,
      });
  });
  return events;
}

function records(value: unknown): Record<string, unknown>[] {
  return Array.isArray(value)
    ? value.flatMap<Record<string, unknown>>((item) => asRecord(item) ?? [])
    : [];
}
