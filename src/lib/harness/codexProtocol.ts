import type { RuntimeMode, ToolPreview } from "../session";
import { composeToolTitle, extractToolPreview } from "./preview";
import { classifyCompletedText } from "./streamText";
import type { HarnessEvent } from "./types";

/** Codex approval / sandbox settings for thread/start and turn/start. */
export type CodexThreadConfig = {
  approvalPolicy: "untrusted" | "on-request" | "never";
  sandbox: "read-only" | "workspace-write" | "danger-full-access";
  approvalsReviewer: "user" | "auto_review";
  sandboxPolicy:
    | { type: "readOnly" }
    | { type: "workspaceWrite" }
    | { type: "dangerFullAccess" };
};

export function runtimeModeToCodexConfig(mode: RuntimeMode): CodexThreadConfig {
  switch (mode) {
    case "supervised":
      return {
        approvalPolicy: "untrusted",
        sandbox: "read-only",
        approvalsReviewer: "user",
        sandboxPolicy: { type: "readOnly" },
      };
    case "auto-accept-edits":
      return {
        approvalPolicy: "on-request",
        sandbox: "workspace-write",
        approvalsReviewer: "user",
        sandboxPolicy: { type: "workspaceWrite" },
      };
    case "auto":
      return {
        approvalPolicy: "on-request",
        sandbox: "workspace-write",
        approvalsReviewer: "auto_review",
        sandboxPolicy: { type: "workspaceWrite" },
      };
    case "full-access":
      return {
        approvalPolicy: "never",
        sandbox: "danger-full-access",
        approvalsReviewer: "user",
        sandboxPolicy: { type: "dangerFullAccess" },
      };
  }
}

export function buildThreadStartParams(input: {
  cwd: string;
  runtimeMode: RuntimeMode;
  model?: string;
  serviceTier?: string;
}): Record<string, unknown> {
  const config = runtimeModeToCodexConfig(input.runtimeMode);
  return {
    cwd: input.cwd,
    approvalPolicy: config.approvalPolicy,
    sandbox: config.sandbox,
    approvalsReviewer: config.approvalsReviewer,
    ...(input.model ? { model: input.model } : {}),
    ...(input.serviceTier && input.serviceTier !== "default"
      ? { serviceTier: input.serviceTier }
      : {}),
  };
}

export function buildTurnSteerParams(input: {
  threadId: string;
  expectedTurnId: string;
  prompt?: string;
  attachments?: Array<{ type: "image"; url: string }>;
}): Record<string, unknown> {
  const turnInput: Array<Record<string, unknown>> = [];
  if (input.prompt) {
    turnInput.push({ type: "text", text: input.prompt });
  }
  for (const attachment of input.attachments ?? []) {
    turnInput.push(attachment);
  }
  return {
    threadId: input.threadId,
    expectedTurnId: input.expectedTurnId,
    input: turnInput,
  };
}

export function buildTurnStartParams(input: {
  threadId: string;
  runtimeMode: RuntimeMode;
  prompt?: string;
  attachments?: Array<{ type: "image"; url: string }>;
  model?: string;
  effort?: string;
  serviceTier?: string;
}): Record<string, unknown> {
  const config = runtimeModeToCodexConfig(input.runtimeMode);
  const turnInput: Array<Record<string, unknown>> = [];
  if (input.prompt) {
    turnInput.push({ type: "text", text: input.prompt });
  }
  for (const attachment of input.attachments ?? []) {
    turnInput.push(attachment);
  }
  return {
    threadId: input.threadId,
    input: turnInput,
    approvalPolicy: config.approvalPolicy,
    approvalsReviewer: config.approvalsReviewer,
    sandboxPolicy: config.sandboxPolicy,
    ...(input.model ? { model: input.model } : {}),
    ...(input.effort ? { effort: input.effort } : {}),
    ...(input.serviceTier && input.serviceTier !== "default"
      ? { serviceTier: input.serviceTier }
      : {}),
  };
}

export function isRecoverableThreadResumeError(error: unknown): boolean {
  const message = (
    error instanceof Error ? error.message : String(error)
  ).toLowerCase();
  if (!message.includes("thread")) return false;
  return [
    "not found",
    "unknown thread",
    "no such thread",
    "does not exist",
    "missing thread",
    "thread id",
  ].some((snippet) => message.includes(snippet));
}

export function asRecord(value: unknown): Record<string, unknown> | null {
  if (value && typeof value === "object" && !Array.isArray(value)) {
    return value as Record<string, unknown>;
  }
  return null;
}

export function stringField(
  rec: Record<string, unknown> | null | undefined,
  key: string,
): string | undefined {
  if (!rec) return undefined;
  const value = rec[key];
  return typeof value === "string" && value.trim() ? value : undefined;
}

export function textField(
  rec: Record<string, unknown> | null | undefined,
  key: string,
): string | undefined {
  if (!rec) return undefined;
  const value = rec[key];
  return typeof value === "string" && value.length > 0 ? value : undefined;
}

function numberField(
  rec: Record<string, unknown> | null | undefined,
  key: string,
): number {
  const value = rec?.[key];
  return typeof value === "number" && Number.isFinite(value) ? value : 0;
}

export type CodexApprovalKind = "command" | "file-change" | "permissions";

export type CodexApprovalDecisionWire =
  "accept" | "acceptForSession" | "decline" | "cancel";

export function toCodexApprovalDecision(
  decision: "allow" | "deny",
  kind: CodexApprovalKind,
): CodexApprovalDecisionWire {
  if (decision === "deny") return "decline";
  // Prefer one-shot accept; session-scoped grants can be added later.
  void kind;
  return "accept";
}

export type CodexTextChannel =
  "assistant" | "reasoning-summary" | "reasoning-content";

export type CodexTextUpdate =
  | {
      kind: "started";
      itemId: string;
      channel: CodexTextChannel;
    }
  | {
      kind: "delta" | "completed";
      itemId: string;
      channel: CodexTextChannel;
      text: string;
    };

type CodexTextLedgerEntry = {
  streamed: string;
};

export type CodexTextState = {
  entries: ReadonlyMap<string, CodexTextLedgerEntry>;
  tailKey: string | null;
};

export type CodexTextUpdateResult = {
  state: CodexTextState;
  event?: Extract<HarnessEvent, { type: "message.delta" | "reasoning.delta" }>;
  conflict?: "position" | "text";
};

export function createCodexTextState(): CodexTextState {
  return { entries: new Map(), tailKey: null };
}

export function invalidateCodexTextTail(state: CodexTextState): CodexTextState {
  return state.tailKey === null ? state : { ...state, tailKey: null };
}

export function applyCodexTextUpdate(
  state: CodexTextState,
  update: CodexTextUpdate,
): CodexTextUpdateResult {
  const key = `${update.itemId}:${update.channel}`;
  const entries = new Map(state.entries);
  if (update.kind === "started") {
    entries.set(key, { streamed: "" });
    return { state: { entries, tailKey: key } };
  }

  const current = entries.get(key);
  if (update.kind === "delta") {
    const streamed = (current?.streamed ?? "") + update.text;
    entries.set(key, { streamed });
    return {
      state: { entries, tailKey: key },
      event: codexTextEvent(update.channel, update.text),
    };
  }

  const relationship = classifyCompletedText({
    streamed: current?.streamed ?? "",
    completed: update.text,
  });
  if (relationship.kind === "already-emitted") return { state };
  if (relationship.kind === "conflict") {
    return { state, conflict: "text" };
  }
  if (!current || state.tailKey !== key) {
    return { state, conflict: "position" };
  }
  const text =
    relationship.kind === "fallback" ? relationship.text : relationship.suffix;
  entries.set(key, { streamed: update.text });
  return {
    state: { entries, tailKey: key },
    ...(text ? { event: codexTextEvent(update.channel, text) } : {}),
  };
}

function codexTextEvent(
  channel: CodexTextChannel,
  text: string,
): Extract<HarnessEvent, { type: "message.delta" | "reasoning.delta" }> {
  return channel === "assistant"
    ? { type: "message.delta", text }
    : { type: "reasoning.delta", text };
}

export type MappedCodexNotification = {
  events: HarnessEvent[];
  textUpdates?: CodexTextUpdate[];
  /** When set, the active turn finished. */
  turnCompleted?: {
    status: "completed" | "failed" | "interrupted" | "cancelled";
    error?: string;
  };
  activeTurnId?: string | null;
  /** Agent message finished but turn/completed has not arrived yet. */
  scheduleTurnWatchdog?: boolean;
};

/**
 * Translate a Codex app-server notification into MonoCode HarnessEvents.
 * Unknown methods return empty events (non-fatal).
 */
export function mapCodexNotification(
  method: string,
  params: unknown,
): MappedCodexNotification {
  const rec = asRecord(params);
  if (!rec) return { events: [] };

  if (method === "item/agentMessage/delta") {
    return mapCodexTextDelta(rec, "assistant");
  }

  if (method === "item/reasoning/summaryTextDelta") {
    return mapCodexTextDelta(rec, "reasoning-summary");
  }

  if (method === "item/reasoning/textDelta") {
    return mapCodexTextDelta(rec, "reasoning-content");
  }

  if (method === "item/plan/delta") {
    const delta = textField(rec, "delta") ?? "";
    if (!delta) return { events: [] };
    return { events: [{ type: "plan", text: delta }] };
  }

  if (method === "turn/plan/updated") {
    const plan = rec.plan;
    if (!Array.isArray(plan)) return { events: [] };
    const text = plan
      .map((step) => {
        const row = asRecord(step);
        const status = stringField(row, "status") ?? "pending";
        const body = stringField(row, "step") ?? "";
        return `${statusMark(status)} ${body}`.trim();
      })
      .filter(Boolean)
      .join("\n");
    if (!text) return { events: [] };
    return { events: [{ type: "plan", text }] };
  }

  if (method === "item/started" || method === "item/completed") {
    return mapItemLifecycle(method, rec);
  }

  if (method === "item/commandExecution/outputDelta") {
    const itemId = stringField(rec, "itemId") ?? "";
    const delta = textField(rec, "delta") ?? "";
    if (!itemId || !delta) return { events: [] };
    return {
      events: [
        {
          type: "tool.updated",
          callId: itemId,
          kind: "execute",
          detail: delta,
          status: "in_progress",
        },
      ],
    };
  }

  if (method === "item/fileChange/patchUpdated") {
    return mapFileChangePatch(rec);
  }

  if (method === "thread/tokenUsage/updated") {
    return mapTokenUsage(rec);
  }

  if (method === "turn/started") {
    const turn = asRecord(rec.turn);
    const turnId = stringField(turn, "id");
    return {
      events: [],
      ...(turnId ? { activeTurnId: turnId } : {}),
    };
  }

  if (method === "turn/completed" || method === "turn/aborted") {
    return mapTurnTerminal(method, rec);
  }

  if (method === "error") {
    const errorObj = asRecord(rec.error);
    const message =
      stringField(errorObj, "message") ??
      stringField(rec, "message") ??
      "Codex error";
    const willRetry = rec.willRetry === true;
    if (willRetry) {
      return { events: [{ type: "status", text: message }] };
    }
    return { events: [{ type: "session.error", message }] };
  }

  if (method === "configWarning" || method === "warning") {
    const message =
      stringField(rec, "summary") ??
      stringField(rec, "message") ??
      stringField(rec, "details");
    if (!message) return { events: [] };
    return { events: [{ type: "status", text: message }] };
  }

  return { events: [] };
}

function mapCodexTextDelta(
  rec: Record<string, unknown>,
  channel: CodexTextChannel,
): MappedCodexNotification {
  const itemId = stringField(rec, "itemId");
  const delta = textField(rec, "delta");
  if (!itemId || !delta) return { events: [] };
  return {
    events: [],
    textUpdates: [{ kind: "delta", itemId, channel, text: delta }],
  };
}

/** Codex thread items MonoCode already renders elsewhere or that are internal metadata. */
const SILENT_ITEM_TYPES = new Set([
  "userMessage",
  "contextCompaction",
  "enteredReviewMode",
  "subAgentActivity",
]);

/**
 * Codex reports both `last` (the most recent request) and `total` (cumulative
 * thread spend). Only `last` describes the context window — `total` keeps
 * climbing across compactions and would run past 100%.
 */
function mapTokenUsage(rec: Record<string, unknown>): MappedCodexNotification {
  const usage = asRecord(rec.tokenUsage);
  const last = asRecord(usage?.last);
  if (!last) return { events: [] };
  const used = numberField(last, "totalTokens");
  const window = numberField(usage, "modelContextWindow");
  if (!used && !window) return { events: [] };
  return {
    events: [
      {
        type: "context",
        ...(used > 0 ? { used } : {}),
        ...(window > 0 ? { window } : {}),
      },
    ],
  };
}

function mapTurnTerminal(
  method: string,
  rec: Record<string, unknown>,
): MappedCodexNotification {
  const turn = asRecord(rec.turn);
  const statusRaw =
    stringField(turn, "status") ??
    (method === "turn/aborted" ? "interrupted" : "completed");
  const errorObj = asRecord(turn?.error);
  const error = stringField(errorObj, "message");
  const status =
    statusRaw === "failed"
      ? "failed"
      : statusRaw === "interrupted" || statusRaw === "cancelled"
        ? statusRaw === "cancelled"
          ? "cancelled"
          : "interrupted"
        : "completed";
  const events: HarnessEvent[] = [
    { type: "message.completed" },
    { type: "reasoning.completed" },
  ];
  if (status === "failed" && error) {
    events.push({ type: "session.error", message: error });
  }
  return {
    events,
    turnCompleted: { status, ...(error ? { error } : {}) },
    activeTurnId: null,
  };
}

function mapItemLifecycle(
  method: string,
  rec: Record<string, unknown>,
): MappedCodexNotification {
  const item = asRecord(rec.item);
  if (!item) return { events: [] };
  const callId = stringField(item, "id") ?? "";
  if (!callId) return { events: [] };
  const itemType = stringField(item, "type") ?? "";
  const completed = method === "item/completed";

  if (SILENT_ITEM_TYPES.has(itemType)) {
    return { events: [] };
  }

  if (itemType === "exitedReviewMode" && completed) {
    const review = stringField(item, "review");
    if (review) {
      return {
        events: [
          { type: "message.delta", text: review },
          { type: "message.completed" },
        ],
      };
    }
    return { events: [] };
  }

  if (itemType === "agentMessage") {
    if (!completed) {
      return {
        events: [],
        textUpdates: [
          { kind: "started", itemId: callId, channel: "assistant" },
        ],
      };
    }
    const text = textField(item, "text");
    return {
      events: [{ type: "message.completed" }],
      ...(text
        ? {
            textUpdates: [
              {
                kind: "completed",
                itemId: callId,
                channel: "assistant",
                text,
              },
            ],
          }
        : {}),
      scheduleTurnWatchdog: true,
    };
  }

  if (itemType === "reasoning") {
    if (!completed) {
      return {
        events: [],
        textUpdates: [
          {
            kind: "started",
            itemId: callId,
            channel: "reasoning-summary",
          },
        ],
      };
    }
    const summary = item.summary;
    const text = Array.isArray(summary)
      ? summary
          .map((part) => {
            if (typeof part === "string") return part;
            return textField(asRecord(part), "text") ?? "";
          })
          .filter(Boolean)
          .join("\n")
      : "";
    return {
      events: [{ type: "reasoning.completed" }],
      ...(text
        ? {
            textUpdates: [
              {
                kind: "completed",
                itemId: callId,
                channel: "reasoning-summary",
                text,
              },
            ],
          }
        : {}),
    };
  }

  if (itemType === "plan") {
    const text = stringField(item, "text");
    if (text) return { events: [{ type: "plan", text }] };
    return { events: [] };
  }

  const mapped = mapToolItem(item, itemType, completed);
  return mapped ? { events: [mapped] } : { events: [] };
}

function mapToolItem(
  item: Record<string, unknown>,
  itemType: string,
  completed: boolean,
): HarnessEvent | null {
  const callId = stringField(item, "id") ?? "";
  if (!callId) return null;

  if (itemType === "commandExecution") {
    const command = stringField(item, "command") ?? "Shell";
    const status = mapItemStatus(stringField(item, "status"), completed);
    const output =
      stringField(item, "aggregatedOutput") ?? stringField(item, "output");
    const preview: ToolPreview | undefined = undefined;
    const eventType = completed ? "tool.updated" : "tool.started";
    if (eventType === "tool.started") {
      return {
        type: "tool.started",
        callId,
        title: command,
        kind: "execute",
        status,
        preview,
      };
    }
    return {
      type: "tool.updated",
      callId,
      title: command,
      kind: "execute",
      status,
      ...(output ? { detail: output } : {}),
      preview,
    };
  }

  if (itemType === "fileChange") {
    return mapFileChangeItem(item, callId, completed);
  }

  if (itemType === "webSearch") {
    const query = stringField(item, "query") ?? "Search";
    const status = mapItemStatus(stringField(item, "status"), completed);
    if (!completed) {
      return {
        type: "tool.started",
        callId,
        title: composeToolTitle({
          kind: "search",
          title: query,
          query,
          previewKind: "search",
        }),
        kind: "search",
        status,
        preview: { kind: "search", query },
      };
    }
    return {
      type: "tool.updated",
      callId,
      title: composeToolTitle({
        kind: "search",
        title: query,
        query,
        previewKind: "search",
      }),
      kind: "search",
      status,
      preview: { kind: "search", query },
    };
  }

  if (itemType === "mcpToolCall") {
    const server = stringField(item, "server") ?? "mcp";
    const tool = stringField(item, "tool") ?? "tool";
    const title = `${server}:${tool}`;
    const status = mapItemStatus(stringField(item, "status"), completed);
    const args = item.arguments;
    const preview =
      extractToolPreview(
        { kind: "other", title, rawInput: args },
        { kind: "other", title, rawInput: args },
      ) ?? undefined;
    if (!completed) {
      return {
        type: "tool.started",
        callId,
        title,
        kind: "other",
        status,
        preview,
      };
    }
    return {
      type: "tool.updated",
      callId,
      title,
      kind: "other",
      status,
      preview,
    };
  }

  // Unknown item types are ignored; Codex may add new internal kinds over time.
  void item;
  void completed;
  return null;
}

function mapFileChangeItem(
  item: Record<string, unknown>,
  callId: string,
  completed: boolean,
): HarnessEvent {
  const changes = Array.isArray(item.changes) ? item.changes : [];
  const first = asRecord(changes[0]);
  const path = stringField(first, "path");
  const diff = stringField(first, "diff");
  const status = mapItemStatus(stringField(item, "status"), completed);
  const preview = buildDiffPreview(path, diff);
  const title =
    composeToolTitle({
      kind: "edit",
      title: path ? `Edit ${path}` : "Edit",
      path,
      previewKind: "write",
    }) || "Edit";
  if (!completed) {
    return {
      type: "tool.started",
      callId,
      title,
      kind: "edit",
      status,
      preview,
    };
  }
  return {
    type: "tool.updated",
    callId,
    title,
    kind: "edit",
    status,
    preview,
  };
}

function mapFileChangePatch(
  rec: Record<string, unknown>,
): MappedCodexNotification {
  const itemId = stringField(rec, "itemId") ?? "";
  if (!itemId) return { events: [] };
  const changes = Array.isArray(rec.changes) ? rec.changes : [];
  const first = asRecord(changes[0]);
  const path = stringField(first, "path");
  const diff = stringField(first, "diff") ?? stringField(rec, "diff");
  const preview = buildDiffPreview(path, diff);
  const title =
    composeToolTitle({
      kind: "edit",
      title: path ? `Edit ${path}` : "Edit",
      path,
      previewKind: "write",
    }) || "Edit";
  return {
    events: [
      {
        type: "tool.updated",
        callId: itemId,
        title,
        kind: "edit",
        status: "in_progress",
        preview,
      },
    ],
  };
}

function buildDiffPreview(
  path: string | undefined,
  diff: string | undefined,
): ToolPreview | undefined {
  if (!path && !diff) return undefined;
  const fake = {
    kind: "edit",
    title: path ? `Edit ${path}` : "Edit",
    content: diff
      ? [{ type: "diff", path, patch: diff }]
      : path
        ? [{ type: "diff", path }]
        : undefined,
    locations: path ? [{ path }] : undefined,
  };
  return (
    extractToolPreview(fake, fake) ??
    (path
      ? { kind: "write", path, fileName: path.split(/[/\\]/).pop() }
      : undefined)
  );
}

function mapItemStatus(status: string | undefined, completed: boolean): string {
  if (status === "completed" || status === "failed" || status === "declined") {
    return status === "declined" ? "failed" : status;
  }
  if (status === "inProgress") return "in_progress";
  return completed ? "completed" : "in_progress";
}

function statusMark(status: string): string {
  if (status === "completed") return "[x]";
  if (status === "inProgress" || status === "in_progress") return "[…]";
  if (status === "cancelled") return "[-]";
  return "[ ]";
}

export function mapApprovalRequest(
  method: string,
  params: unknown,
  requestId: number,
): {
  kind: CodexApprovalKind;
  event: Extract<HarnessEvent, { type: "approval.requested" }>;
} | null {
  const rec = asRecord(params);
  if (!rec) return null;

  if (method === "item/commandExecution/requestApproval") {
    const command = stringField(rec, "command") ?? "Shell";
    const callId = stringField(rec, "itemId");
    const reason = stringField(rec, "reason");
    return {
      kind: "command",
      event: {
        type: "approval.requested",
        requestId,
        title: reason ? `${command} — ${reason}` : command,
        kind: "execute",
        callId,
        preview: undefined,
      },
    };
  }

  if (method === "item/fileChange/requestApproval") {
    const callId = stringField(rec, "itemId");
    const reason = stringField(rec, "reason");
    const title = reason ?? "Approve file changes";
    return {
      kind: "file-change",
      event: {
        type: "approval.requested",
        requestId,
        title,
        kind: "edit",
        callId,
      },
    };
  }

  if (method === "item/permissions/requestApproval") {
    const callId = stringField(rec, "itemId");
    const reason = stringField(rec, "reason") ?? "Approve permissions";
    return {
      kind: "permissions",
      event: {
        type: "approval.requested",
        requestId,
        title: reason,
        kind: "other",
        callId,
      },
    };
  }

  return null;
}
