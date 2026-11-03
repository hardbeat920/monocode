import type { HarnessEvent } from "../../core/types";
import { asRecord } from "../antigravity/antigravityProtocol";

type Spawn = {
  title?: string;
  task?: string;
  /** The `run_subagent` call that spawned this one, when it was nested. */
  parent?: string;
  agentId?: string;
  settled?: boolean;
};

/**
 * Devin names a subagent by its own agent id, not by the `run_subagent` call
 * that spawned it: `cognition.ai/subagent_started` arrives as a bare update
 * keyed by that id, the child's own updates carry
 * `cognition.ai/subagent_context.parentAgentId`, and
 * `cognition.ai/subagent_completed` settles it. This ties each agent id back
 * to its spawn call so the call becomes the agent row and the child's work is
 * attributed to it through the usual `parentToolCallId`, the way
 * `AcpSubagents` already handles other ACP servers.
 */
export class DevinSubagents {
  private spawns = new Map<string, Spawn>();
  private agents = new Map<string, string>();

  /** Params with Devin's parent identity rewritten, and the events to route. */
  translate(
    params: unknown,
    events: HarnessEvent[],
  ): { params: unknown; events: HarnessEvent[] } {
    const envelope = asRecord(params);
    const update = asRecord(envelope?.update) ?? envelope;
    const meta = asRecord(update?._meta);
    if (!update || !meta) return { params, events };

    const started = asRecord(meta["cognition.ai/subagent_started"]);
    const startedId = field(started, "agentId");
    if (started && startedId) {
      const callId = this.link(startedId, started);
      const spawn = this.spawns.get(callId);
      const title = field(started, "title") ?? spawn?.title;
      return this.lifecycle(params, callId, {
        type: "tool.updated",
        callId,
        kind: "agent",
        ...(title ? { title } : {}),
        status: "in_progress",
      });
    }

    const completed = asRecord(meta["cognition.ai/subagent_completed"]);
    const completedId = field(completed, "agentId");
    if (completed && completedId) {
      const callId = this.agents.get(completedId) ?? completedId;
      const spawn = this.spawns.get(callId);
      if (spawn) spawn.settled = true;
      const failed = completed.success === false;
      const summary = field(completed, "summary");
      return this.lifecycle(params, callId, {
        type: "tool.updated",
        callId,
        kind: "agent",
        status: failed ? "failed" : "completed",
        ...(failed && summary ? { detail: summary } : {}),
      });
    }

    const context = asRecord(meta["cognition.ai/subagent_context"]);
    const parentAgentId = field(context, "parentAgentId");
    const parent = parentAgentId
      ? (this.agents.get(parentAgentId) ?? parentAgentId)
      : undefined;
    const callId = toolCallId(update);
    if (meta["cognition.ai/inferenceToolName"] === "run_subagent" && callId) {
      events = this.spawn(callId, update, parent, events);
    }
    if (!parent) return { params, events };
    return { params: withParent(params, parent), events };
  }

  /** A `run_subagent` call is the agent row; returning is not finishing. */
  private spawn(
    callId: string,
    update: Record<string, unknown>,
    parent: string | undefined,
    events: HarnessEvent[],
  ): HarnessEvent[] {
    const input = asRecord(update.rawInput) ?? asRecord(update.raw_input);
    const spawn = this.spawns.get(callId) ?? {};
    spawn.title = field(input, "title") ?? spawn.title;
    spawn.task = field(input, "task") ?? spawn.task;
    spawn.parent = parent ?? spawn.parent;
    this.spawns.set(callId, spawn);
    if (this.spawns.size > 256) {
      const oldest = this.spawns.keys().next().value!;
      this.spawns.delete(oldest);
    }
    return events.map((event) => {
      if (event.type !== "tool.updated" || event.callId !== callId) return event;
      // A background spawn returns at once; a foreground one returns with its
      // result. Either way the agent's own completion settles the row, unless
      // no agent ever started (an older CLI without subagent metadata).
      const pending =
        !!spawn.agentId && !spawn.settled && event.status === "completed";
      // The call's own output is a receipt ("Background subagent started."),
      // not the agent's work; only a failed spawn has something to show.
      const title = spawn.title ?? event.title;
      const status = pending ? "in_progress" : event.status;
      return {
        type: "tool.updated",
        callId,
        kind: "agent",
        ...(title ? { title } : {}),
        ...(status ? { status } : {}),
        ...(status === "failed" && event.detail ? { detail: event.detail } : {}),
      };
    });
  }

  /**
   * The spawn call this agent belongs to: the one with the same brief, else
   * the newest spawn not yet tied to an agent. Without one, the agent id
   * itself names the row, as Devin's own client does.
   */
  private link(agentId: string, started: Record<string, unknown>): string {
    const known = this.agents.get(agentId);
    const title = field(started, "title");
    const task = field(started, "task");
    const free = [...this.spawns].filter(([, spawn]) => !spawn.agentId);
    const match =
      free.find(([, spawn]) => spawn.title === title && spawn.task === task) ??
      free[free.length - 1];
    // A resumed agent keeps its id but is spawned by a new call.
    const callId = match?.[0] ?? known ?? agentId;
    const spawn = this.spawns.get(callId);
    if (spawn) {
      spawn.agentId = agentId;
      spawn.settled = false;
    }
    this.agents.set(agentId, callId);
    return callId;
  }

  /** Lifecycle updates belong to the spawn's own parent when it is nested. */
  private lifecycle(
    params: unknown,
    callId: string,
    event: HarnessEvent,
  ): { params: unknown; events: HarnessEvent[] } {
    const parent = this.spawns.get(callId)?.parent;
    return {
      params: parent ? withParent(params, parent) : params,
      events: [event],
    };
  }
}

function toolCallId(update: Record<string, unknown>): string | undefined {
  return field(update, "toolCallId") ?? field(update, "tool_call_id");
}

function withParent(params: unknown, parent: string): unknown {
  const envelope = asRecord(params);
  const update = asRecord(envelope?.update);
  if (!envelope || !update) return params;
  return {
    ...envelope,
    update: {
      ...update,
      _meta: { ...asRecord(update._meta), parentToolCallId: parent },
    },
  };
}

function field(
  rec: Record<string, unknown> | null | undefined,
  key: string,
): string | undefined {
  const value = rec?.[key];
  return typeof value === "string" && value.trim() ? value : undefined;
}
