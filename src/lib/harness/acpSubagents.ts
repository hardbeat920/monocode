import type { HarnessEvent } from "./types";
import {
  agentToolTitle,
  isAgentTool,
  isAgentToolName,
  mergeToolPreview,
} from "./preview";

type Step = Extract<HarnessEvent, { type: "agent.step" }>;

/** Route explicitly attributed ACP child activity without mixing parent text. */
export class AcpSubagents {
  private tools = new Set<string>();
  private owners = new Map<string, string>();
  private pending = new Map<string, Step[]>();
  private prose = new Map<
    string,
    { id: number; kind: "message" | "reasoning"; text: string }
  >();
  private sequence = 0;

  isChild(params: unknown): boolean {
    return !!this.parent(params);
  }

  route(params: unknown, events: HarnessEvent[]): HarnessEvent[] {
    const parent = this.parent(params);
    if (!parent) {
      return events.flatMap<HarnessEvent>((event) => {
        if (event.type !== "tool.started" && event.type !== "tool.updated")
          return [event];
        this.tools.add(event.callId);
        const backlog = this.pending.get(event.callId) ?? [];
        this.pending.delete(event.callId);
        return [event, ...backlog];
      });
    }
    const output: Step[] = [];
    for (const event of events) {
      if (event.type === "tool.started" || event.type === "tool.updated") {
        this.owners.set(event.callId, parent);
        this.prose.delete(parent);
        output.push({
          type: "agent.step",
          callId: parent,
          stepId: `tool:${event.callId}`,
          kind: "tool",
          text: event.title ?? "",
          toolKind: event.kind,
          status: event.status,
          preview: event.preview,
        });
      } else if (
        event.type === "message.delta" ||
        event.type === "reasoning.delta"
      ) {
        const kind = event.type === "message.delta" ? "message" : "reasoning";
        let prose = this.prose.get(parent);
        if (!prose || prose.kind !== kind)
          prose = { id: ++this.sequence, kind, text: "" };
        const update = record(record(params)?.update) ?? record(params);
        const type =
          update?.sessionUpdate ?? update?.session_update ?? update?.type;
        const snapshot = type === "agent_message" || type === "agent_thought";
        prose.text = (snapshot ? event.text : prose.text + event.text).slice(
          0,
          2_000,
        );
        this.prose.set(parent, prose);
        output.push({
          type: "agent.step",
          callId: parent,
          stepId: `${kind}:${prose.id}`,
          kind,
          text: prose.text,
        });
      }
      // Child plans, context meters and lifecycle notifications belong to the
      // child too; they must never replace or finish the parent's own work.
    }
    if (this.tools.has(parent)) return output;
    const backlog = this.pending.get(parent) ?? [];
    for (const step of output) {
      const index = backlog.findIndex((entry) => entry.stepId === step.stepId);
      if (index < 0) backlog.push(step);
      else
        backlog[index] = {
          ...backlog[index],
          ...step,
          text: step.text || backlog[index].text,
          toolKind: step.toolKind ?? backlog[index].toolKind,
          status: step.status ?? backlog[index].status,
          preview: mergeToolPreview(step.preview, backlog[index].preview),
        };
    }
    this.pending.set(parent, backlog.slice(-64));
    if (this.pending.size > 32)
      this.pending.delete(this.pending.keys().next().value!);
    return [];
  }

  private parent(params: unknown): string | undefined {
    const envelope = record(params);
    const update = record(envelope?.update) ?? envelope;
    const tool = record(update?.toolCall) ?? record(update?.tool_call);
    const sources = [tool, update, envelope];
    const id = text(
      tool?.toolCallId ??
        tool?.tool_call_id ??
        update?.toolCallId ??
        update?.tool_call_id,
    );
    let parent: string | undefined;
    for (const source of sources) {
      const meta = record(source?._meta);
      for (const entry of [
        source,
        meta,
        record(meta?.cursor),
        record(meta?.grok),
        record(meta?.["x.ai"]),
        record(meta?.fx),
      ]) {
        parent = text(entry?.parentToolCallId ?? entry?.parent_tool_call_id);
        if (parent) break;
      }
      if (parent) break;
    }
    parent ??= id ? this.owners.get(id) : undefined;
    if (!parent || parent === id) return undefined;
    const seen = new Set<string>();
    while (this.owners.has(parent) && !seen.has(parent)) {
      seen.add(parent);
      parent = this.owners.get(parent)!;
    }
    return seen.has(parent) ? undefined : parent;
  }
}

function record(value: unknown): Record<string, unknown> | undefined {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : undefined;
}

function text(value: unknown): string | undefined {
  return typeof value === "string" && value.trim() ? value : undefined;
}

/** Some ACP servers classify delegation as `other` and identify it in input. */
export function acpAgentInfo(
  update: Record<string, unknown>,
  tool: Record<string, unknown>,
  kind?: string,
  title?: string,
  nativeInput?: unknown,
): { kind: "agent"; title: string } | undefined {
  const input = record(
    nativeInput ??
      update.rawInput ??
      tool.rawInput ??
      update.raw_input ??
      tool.raw_input ??
      update.input ??
      tool.input,
  );
  const name = text(
    input?._toolName ?? input?.toolName ?? update.name ?? tool.name,
  );
  if (!isAgentTool(kind, title) && !(name && isAgentToolName(name)))
    return undefined;
  return { kind: "agent", title: agentToolTitle(input ?? {}, title) };
}
