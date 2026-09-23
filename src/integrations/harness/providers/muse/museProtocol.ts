import type { RuntimeMode } from "../../../../features/sessions/model/session";
import type { Attachment } from "../../../../features/sessions/model/session";
import type { HarnessEvent } from "../../core/types";

export const MUSE_AUTH_HELP =
  "Muse CLI not signed in. Run `muse login` in a terminal (or set META_API_KEY), then retry.";

const EFFORTS = new Set([
  "none",
  "minimal",
  "low",
  "medium",
  "high",
  "xhigh",
  "max",
  "ultra",
]);

/** Reasoning tier from model settings, mirroring the grok `effort` key. */
export function museEffort(settings?: Record<string, string>): string | undefined {
  const value =
    settings?.effort?.trim() || settings?.reasoning?.trim() || settings?.reasoningEffort?.trim();
  if (!value || !EFFORTS.has(value)) return undefined;
  return value;
}

/** Map MonoCode access levels to `muse exec` approval flags. The OS sandbox
 * stays on: full-access only relaxes the approval prompts, never the sandbox. */
export function museApprovalArgs(runtimeMode: RuntimeMode): string[] {
  if (runtimeMode === "full-access") return ["--approval-mode", "never"];
  return [];
}

export type MusePrompt = {
  text: string;
  /** Local image files passed via repeatable `--image`. */
  images: string[];
};

/** Split attachments into prompt text plus native `--image` paths. File
 * attachments ride along as `@path` mentions; images without a disk path and
 * pasted blobs cannot be materialized and are skipped. */
export function musePromptParts(
  text: string,
  attachments: readonly Attachment[] | undefined,
  cwd: string,
): MusePrompt {
  const images: string[] = [];
  const mentions: string[] = [];
  const dropped: string[] = [];
  for (const attachment of attachments ?? []) {
    if (attachment.kind === "image" && attachment.path) {
      images.push(attachment.path);
      continue;
    }
    if (attachment.path) {
      mentions.push(mentionFor(cwd, attachment.path));
      continue;
    }
    dropped.push(attachment.name || attachment.id);
  }
  const lines = mentions.length > 0 ? ["", "", "Attached files:", ...mentions] : [];
  if (dropped.length > 0) {
    lines.push(
      "",
      `[Note: ${dropped.length} attachment(s) could not be attached (no file path): ${dropped.join(", ")}]`,
    );
  }
  return { text: `${text}${lines.join("\n")}`, images };
}

function mentionFor(cwd: string, path: string): string {
  const cleanCwd = cwd.replace(/\/+$/, "");
  if (cleanCwd && path.startsWith(`${cleanCwd}/`)) {
    return `@${path.slice(cleanCwd.length + 1)}`;
  }
  return `@${path}`;
}

/** Argv for one headless `muse exec --json` turn. Resume passes the stored
 * Muse session id; the first turn omits it and the server mints one. */
export function museSpawnArgs(options: {
  cwd: string;
  prompt: string;
  images: readonly string[];
  resumeSessionId?: string;
  modelId?: string;
  effort?: string;
  runtimeMode: RuntimeMode;
}): string[] {
  const args = ["exec", "--json", "--workspace", options.cwd];
  if (options.resumeSessionId) {
    args.push("--session-id", options.resumeSessionId);
  }
  const model = options.modelId?.trim();
  if (model) args.push("--model", model);
  if (options.effort) args.push("--reasoning-effort", options.effort);
  args.push(...museApprovalArgs(options.runtimeMode));
  for (const image of options.images) args.push("--image", image);
  args.push(options.prompt);
  return args;
}

export function museAuthError(message: string): boolean {
  return [
    /\bnot (?:authenticated|signed in|logged in)\b/i,
    /\bcredentials?\b.*\b(?:missing|expired|invalid)\b/i,
    /\bMETA_API_KEY\b/i,
    /\bplease (?:sign|log) in\b/i,
    /\brun [`'"]?muse login\b/i,
  ].some((pattern) => pattern.test(message));
}

/** Human title for a `tool.<name>` task kind. */
export function toolTitleForTaskKind(taskKind: string): string {
  const name = taskKind.startsWith("tool.") ? taskKind.slice("tool.".length) : taskKind;
  return name.trim() || taskKind;
}

type ExecRecord = {
  payload_type?: string;
  stream?: { kind?: string; id?: string };
  payload?: Record<string, unknown>;
};

function asRecord(value: unknown): Record<string, unknown> | undefined {
  if (typeof value !== "object" || value === null) return undefined;
  return value as Record<string, unknown>;
}

function textField(record: Record<string, unknown>, key: string): string | undefined {
  const value = record[key];
  return typeof value === "string" && value ? value : undefined;
}

/**
 * Fold one headless `muse exec --json` turn into MonoCode harness events.
 * MSP serve and exec share the envelope; only the exec payload subset is
 * mapped here, unknown payload types are ignored so schema additions cannot
 * break the transcript.
 */
export class MuseExecFold {
  sessionId: string | undefined;
  done = false;
  failed: string | undefined;
  private tools = new Map<string, string>();
  private callToTask = new Map<string, string>();

  pushLine(line: string): HarnessEvent[] {
    let record: ExecRecord;
    try {
      record = JSON.parse(line) as ExecRecord;
    } catch {
      return [];
    }
    // Only the session stream id is resumable via `--session-id`; run/task
    // streams multiplexed inside the payload must never be adopted.
    if (record.stream?.kind === "session" && record.stream.id && !this.sessionId) {
      this.sessionId = record.stream.id;
    }
    const payload = asRecord(record.payload);
    if (!payload) return [];
    switch (record.payload_type) {
      case "run.output.delta": {
        const text = textField(payload, "text");
        return text ? [{ type: "message.delta", text }] : [];
      }
      case "run.terminal.completed":
      case "run.terminal.cancelled":
      case "run.terminal.failed": {
        this.done = true;
        const terminal = textField(payload, "terminal");
        const events: HarnessEvent[] = [{ type: "message.completed" }];
        if (terminal && terminal !== "completed" && terminal !== "cancelled") {
          const reason = textField(payload, "reason") ?? textField(payload, "text");
          this.failed = `Muse turn ended (${terminal})${reason ? `: ${reason}` : ""}`;
          events.push({ type: "session.error", message: this.failed });
        }
        return events;
      }
      case "task.lifecycle.proposed":
      case "task.lifecycle.accepted":
      case "task.lifecycle.scheduled":
      case "task.lifecycle.started": {
        const event = asRecord(payload.event);
        const taskId = textField(payload, "task_id") ?? textField(event ?? {}, "task_id");
        const taskKind = event ? textField(event, "task_kind") : undefined;
        if (!taskId || !taskKind || !taskKind.startsWith("tool.")) return [];
        const known = this.tools.has(taskId);
        this.tools.set(taskId, toolTitleForTaskKind(taskKind));
        if (known) return [];
        return [
          { type: "tool.started", callId: taskId, title: toolTitleForTaskKind(taskKind) },
        ];
      }
      case "task.lifecycle.status":
      case "task.lifecycle.output": {
        const event = asRecord(payload.event);
        const taskId = textField(payload, "task_id");
        if (!taskId || !this.tools.has(taskId) || !event) return [];
        const detail = textField(event, "message") ?? textField(event, "chunk");
        return detail ? [{ type: "tool.updated", callId: taskId, detail }] : [];
      }
      case "task.lifecycle.tool_output_ref": {
        const event = asRecord(payload.event);
        const taskId = textField(payload, "task_id");
        const ref = asRecord(event?.output_ref);
        const refId = ref ? textField(ref, "id") : undefined;
        if (!taskId || !refId) return [];
        const callId = refId.match(/call_[A-Za-z0-9_-]+/)?.[0];
        if (callId) this.callToTask.set(callId, taskId);
        return [];
      }
      case "tool.result": {
        const callId = textField(payload, "call_id");
        const facts = asRecord(payload.correlation_facts) ?? asRecord(payload.edit_facts);
        const toolName = facts ? textField(facts, "tool_name") : undefined;
        const path = asRecord(payload.edit_facts)?.path;
        const taskId = callId ? this.callToTask.get(callId) : undefined;
        const target = taskId ?? callId;
        if (!target) return [];
        return [
          {
            type: "tool.updated",
            callId: target,
            title: toolName,
            status: "completed",
            detail: textField(payload, "text"),
            paths: typeof path === "string" && path ? [path] : undefined,
          },
        ];
      }
      default:
        return [];
    }
  }
}

export function museStartupError(error: unknown): Error {
  const detail = error instanceof Error ? error.message : String(error);
  if (museAuthError(detail)) {
    return new Error(`${detail.trim()}\n\n${MUSE_AUTH_HELP}`);
  }
  if (/timed out/i.test(detail)) {
    return new Error(`Muse did not start. ${MUSE_AUTH_HELP}`);
  }
  return new Error(`Muse did not start. ${detail}`);
}
