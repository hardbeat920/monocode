import type {
  ModEventName,
  SessionInfo,
  ToolCallEvent,
  ToolCallResult,
  ToolName,
} from "../api/types";
import type { HarnessEvent } from "../../../integrations/harness/core/types";
import type { Session, TurnMetrics } from "../../sessions/model/session";

export type ModEmission =
  | { type: "event"; name: ModEventName; event: unknown }
  | { type: "tool.done"; callId: string; result: ToolCallResult };

type ToolStart = Extract<HarnessEvent, { type: "tool.started" }>;

const TERMINAL = new Set([
  "completed",
  "success",
  "failed",
  "error",
  "cancelled",
  "canceled",
]);
const FAILED = new Set(["failed", "error", "cancelled", "canceled"]);

function toolResult(
  status: string | undefined,
  output: string | undefined,
): ToolCallResult {
  return {
    isError: FAILED.has(status?.toLowerCase() ?? ""),
    result: output === undefined ? undefined : { stdout: output },
  };
}

/** Other providers' tool kinds, normalised onto Claude tool names. */
function toolName(start: ToolStart): ToolName {
  switch (start.kind) {
    case "execute":
      return "Bash";
    case "edit":
      return start.preview?.contentOnly ? "Write" : "Edit";
    case "read":
      return "Read";
    case "search":
      return "Grep";
    case "agent":
      return "Agent";
    default:
      return start.kind ?? start.title;
  }
}

export function sessionInfo(session: Session): SessionInfo {
  return {
    id: session.id,
    cwd: session.worktreeCwd ?? session.cwd,
    projectCwd: session.cwd,
    provider: session.harness,
    isInteractive:
      !session.ephemeral && !session.orchestrationLeadId && !session.inboxAsk,
  };
}

export function toolCallEvent(
  sessionId: string,
  start: ToolStart,
): ToolCallEvent {
  const tool = toolName(start);
  const path = start.preview?.path;
  return {
    sessionId,
    callId: start.callId,
    tool,
    // Providers put the command in the row title; it can be a readable rewrite.
    command: tool === "Bash" ? start.title : undefined,
    file_path: path,
    paths: start.paths ?? (path ? [path] : undefined),
  };
}

/**
 * Turns harness flush batches and session-state changes into mod events.
 * Tracks turn start times, metrics and pending tool calls.
 */
export class ModEventTap {
  private seen = new Set<string>();
  private turnStarts = new Map<string, number>();
  private metrics = new Map<string, TurnMetrics>();
  /** callId -> sessionId and the latest output, until a terminal status. */
  private pending = new Map<string, { sessionId: string; output?: string }>();

  derive(
    batches: ReadonlyMap<string, readonly HarnessEvent[]>,
    next: readonly Session[],
  ): ModEmission[] {
    const out: ModEmission[] = [];
    const emit = (name: ModEventName, event: unknown) =>
      out.push({ type: "event", name, event });
    const done = (callId: string, status: string | undefined) => {
      const call = this.pending.get(callId);
      if (!call) return;
      this.pending.delete(callId);
      out.push({ type: "tool.done", callId, result: toolResult(status, call.output) });
    };

    for (const [sessionId, events] of batches) {
      const after = next.find((s) => s.id === sessionId);
      if (after) this.start(after, out);

      for (const event of events) {
        switch (event.type) {
          case "context":
            emit("context.update", {
              sessionId,
              used: event.used,
              window: event.window,
            });
            break;
          case "turn.metrics": {
            const { type: _, ...metrics } = event;
            this.metrics.set(sessionId, {
              ...this.metrics.get(sessionId),
              ...metrics,
            });
            break;
          }
          case "tool.started":
            if (this.pending.has(event.callId)) break;
            this.pending.set(event.callId, {
              sessionId,
              output: event.preview?.output,
            });
            emit("tool.call", toolCallEvent(sessionId, event));
            if (TERMINAL.has(event.status?.toLowerCase() ?? ""))
              done(event.callId, event.status);
            break;
          case "tool.updated": {
            const call = this.pending.get(event.callId);
            if (!call) break;
            if (event.preview?.output !== undefined)
              call.output = event.preview.output;
            if (TERMINAL.has(event.status?.toLowerCase() ?? ""))
              done(event.callId, event.status);
            break;
          }
          // A subagent's tool call, mirrored onto its parent Agent call: `agentId` is that call's id.
          case "agent.step": {
            if (event.kind !== "tool") break;
            const call = this.pending.get(event.stepId);
            if (!call) {
              this.pending.set(event.stepId, {
                sessionId,
                output: event.preview?.output,
              });
              emit("tool.call", {
                ...toolCallEvent(sessionId, {
                  type: "tool.started",
                  callId: event.stepId,
                  title: event.text,
                  kind: event.toolKind,
                  preview: event.preview,
                }),
                agentId: event.callId,
              });
            } else if (event.preview?.output !== undefined) {
              call.output = event.preview.output;
            }
            if (TERMINAL.has(event.status?.toLowerCase() ?? ""))
              done(event.stepId, event.status);
            break;
          }
          case "session.ended":
            emit("session.end", { sessionId });
            this.seen.delete(sessionId);
            break;
        }
      }
    }
    return out;
  }

  /**
   * Turn events from `busy` flips. Turns start and stop outside the harness
   * flush (the composer and the turn's finally block set `busy`), so App
   * calls this on every session-state change, not only on flushes.
   */
  turns(
    previous: readonly Session[],
    next: readonly Session[],
    now = Date.now(),
  ): ModEmission[] {
    const out: ModEmission[] = [];
    const before = new Map(previous.map((s) => [s.id, s]));
    for (const after of next) {
      const wasBusy = !!before.get(after.id)?.busy;
      if (!wasBusy && after.busy) {
        this.start(after, out);
        this.turnStarts.set(after.id, now);
        this.metrics.delete(after.id);
        out.push({ type: "event", name: "turn.start", event: { sessionId: after.id } });
      } else if (wasBusy && !after.busy) {
        this.complete(after, out, now);
      }
    }
    return out;
  }

  private start(session: Session, out: ModEmission[]) {
    if (this.seen.has(session.id)) return;
    this.seen.add(session.id);
    out.push({ type: "event", name: "session.start", event: sessionInfo(session) });
  }

  private complete(session: Session, out: ModEmission[], now: number) {
    // Calls the turn left open are settled from the final transcript.
    for (const [callId, call] of this.pending) {
      if (call.sessionId !== session.id) continue;
      const block = session.blocks.find((b) => b.tool?.callId === callId);
      this.pending.delete(callId);
      out.push({
        type: "tool.done",
        callId,
        result: toolResult(block?.tool?.status ?? "cancelled", call.output),
      });
    }
    const started = this.turnStarts.get(session.id);
    const metrics = this.metrics.get(session.id);
    this.turnStarts.delete(session.id);
    this.metrics.delete(session.id);
    out.push({
      type: "event",
      name: "turn.complete",
      event: {
        sessionId: session.id,
        durationMs: started === undefined ? 0 : now - started,
        metrics: metrics && {
          inputTokens: metrics.inputTokens,
          outputTokens: metrics.outputTokens,
          cacheReadTokens: metrics.cacheReadTokens,
          cacheWriteTokens: metrics.cacheWriteTokens,
        },
      },
    });
  }
}
