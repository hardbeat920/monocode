import type { Attachment, RuntimeMode, ToolPreview } from "../session";
import { composeToolTitle, extractToolPreview } from "./preview";
import { classifyCompletedText } from "./streamText";
import type { ApprovalDecision, HarnessEvent } from "./types";

/** Claude Code versions that first ship Opus 5 / Fable 5 / Opus 4.8 / 4.7. */
export const MINIMUM_CLAUDE_OPUS_5_VERSION = "2.1.219";
export const MINIMUM_CLAUDE_FABLE_5_VERSION = "2.1.169";
export const MINIMUM_CLAUDE_OPUS_4_8_VERSION = "2.1.154";
export const MINIMUM_CLAUDE_OPUS_4_7_VERSION = "2.1.111";

export const CLAUDE_SETTING_SOURCES = "user,project,local";

export const SUPPORTED_CLAUDE_IMAGE_MIME_TYPES = new Set([
  "image/gif",
  "image/jpeg",
  "image/png",
  "image/webp",
]);

export type ClaudePermissionMode =
  "default" | "acceptEdits" | "auto" | "bypassPermissions";

export type ClaudeControlRequest = {
  requestId: string;
  subtype: string;
  toolName?: string;
  input?: Record<string, unknown>;
  toolUseId?: string;
};

export type ClaudeMappedLine = {
  events: HarnessEvent[];
  sessionId?: string;
  control?: ClaudeControlRequest;
  cancelRequestId?: string;
  turnCompleted?: {
    status: "completed" | "failed" | "interrupted" | "cancelled";
    error?: string;
  };
};

export type ClaudeCliSettings = {
  alwaysThinkingEnabled?: boolean;
  fastMode?: boolean;
  ultracode?: boolean;
  disableAllHooks?: boolean;
};

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

export function parseClaudeVersion(output: string): string | null {
  const match = output.match(/\d+\.\d+\.\d+/);
  return match?.[0] ?? null;
}

export function compareSemver(left: string, right: string): number {
  const a = left.split(".").map((part) => Number.parseInt(part, 10) || 0);
  const b = right.split(".").map((part) => Number.parseInt(part, 10) || 0);
  for (let i = 0; i < 3; i += 1) {
    const delta = (a[i] ?? 0) - (b[i] ?? 0);
    if (delta !== 0) return delta;
  }
  return 0;
}

export function runtimeModeToPermission(
  mode: RuntimeMode,
): ClaudePermissionMode | undefined {
  switch (mode) {
    case "auto-accept-edits":
      return "acceptEdits";
    case "auto":
      return "auto";
    case "full-access":
      return "bypassPermissions";
    default:
      return undefined;
  }
}

/**
 * Normalize a resolved Claude effort for `--effort`.
 * `ultracode` pairs with `xhigh`; `ultrathink` is a prompt prefix, not a CLI effort.
 */
export function normalizeClaudeCliEffort(
  effort: string | null | undefined,
  model: string | null | undefined,
): string | undefined {
  if (!effort || effort === "ultrathink") return undefined;
  if (effort === "ultracode") return "xhigh";
  if (
    effort === "xhigh" &&
    model !== "claude-fable-5" &&
    model !== "claude-opus-5" &&
    model !== "claude-opus-4-8" &&
    model !== "claude-sonnet-5"
  ) {
    return "max";
  }
  if (effort === "max" && model === "claude-sonnet-4-6") return "high";
  return effort;
}

export function isClaudeUltracodeEffort(
  effort: string | null | undefined,
): boolean {
  return effort === "ultracode";
}

export function applyClaudePromptEffortPrefix(
  text: string,
  effort: string | null | undefined,
): string {
  if (effort !== "ultrathink") return text;
  if (!text) return "Ultrathink:";
  return `Ultrathink:\n${text}`;
}

export function resolveClaudeApiModelId(
  model: string,
  context?: string | null,
): string {
  if (context === "1m") return `${model}[1m]`;
  return model;
}

export function parseJsonLine(line: string): Record<string, unknown> | null {
  const trimmed = line.trim();
  if (!trimmed.startsWith("{")) return null;
  try {
    return asRecord(JSON.parse(trimmed));
  } catch {
    return null;
  }
}

export function buildClaudeUserMessage(input: {
  text: string;
  attachments?: Attachment[];
  effort?: string | null;
}): Record<string, unknown> {
  const text = applyClaudePromptEffortPrefix(input.text.trim(), input.effort);
  const content: Array<Record<string, unknown>> = [];
  if (text) content.push({ type: "text", text });
  for (const attachment of input.attachments ?? []) {
    const block = imageContentBlock(attachment);
    if (block) content.push(block);
  }
  return {
    type: "user",
    session_id: "",
    parent_tool_use_id: null,
    message: {
      role: "user",
      content,
    },
  };
}

function imageContentBlock(
  attachment: Attachment,
): Record<string, unknown> | null {
  if (attachment.kind !== "image" || !attachment.data) return null;
  const mime = normalizeImageMime(attachment.mimeType);
  if (!SUPPORTED_CLAUDE_IMAGE_MIME_TYPES.has(mime)) return null;
  return {
    type: "image",
    source: {
      type: "base64",
      media_type: mime,
      data: attachment.data,
    },
  };
}

function normalizeImageMime(mime: string): string {
  if (mime === "image/jpg") return "image/jpeg";
  return mime;
}

export function buildClaudeSpawnArgs(input: {
  model?: string;
  effort?: string;
  permissionMode?: ClaudePermissionMode;
  resume?: string;
  sessionId?: string;
  settings?: ClaudeCliSettings;
  includePartialMessages?: boolean;
  maxTurns?: number;
  isolated?: boolean;
}): string[] {
  const args = [
    "--output-format",
    "stream-json",
    "--verbose",
    "--input-format",
    "stream-json",
  ];
  if (!input.isolated) {
    args.push("--permission-prompt-tool", "stdio");
  }
  if (input.includePartialMessages !== false) {
    args.push("--include-partial-messages");
  }
  // PermissionRequest hooks run in parallel with the stdio prompt and dismiss
  // it when they return allow/deny. MonoCode is the approval UI, so hooks
  const settings = {
    disableAllHooks: true,
    ...input.settings,
  };
  if (input.isolated) {
    args.push("--no-session-persistence");
    args.push("--strict-mcp-config");
    args.push("--mcp-config", JSON.stringify({ mcpServers: {} }));
    args.push("--settings", JSON.stringify(settings));
  } else {
    args.push(`--setting-sources=${CLAUDE_SETTING_SOURCES}`);
    args.push("--settings", JSON.stringify(settings));
  }
  if (input.model) args.push("--model", input.model);
  if (input.effort) args.push("--effort", input.effort);
  if (input.permissionMode) {
    args.push("--permission-mode", input.permissionMode);
  }
  if (input.permissionMode === "bypassPermissions") {
    args.push("--allow-dangerously-skip-permissions");
  }
  if (input.resume) args.push("--resume", input.resume);
  if (input.sessionId) args.push("--session-id", input.sessionId);
  if (input.maxTurns) args.push("--max-turns", String(input.maxTurns));
  return args;
}

export function buildControlRequest(
  requestId: string,
  request: Record<string, unknown>,
): Record<string, unknown> {
  return {
    type: "control_request",
    request_id: requestId,
    request,
  };
}

export function buildControlResponse(
  requestId: string,
  response: Record<string, unknown>,
): Record<string, unknown> {
  return {
    type: "control_response",
    response: {
      subtype: "success",
      request_id: requestId,
      response,
    },
  };
}

export function toClaudePermissionResult(
  decision: ApprovalDecision,
  input: Record<string, unknown>,
): Record<string, unknown> {
  if (decision === "allow") {
    return { behavior: "allow", updatedInput: input };
  }
  return {
    behavior: "deny",
    message: "User declined tool execution.",
  };
}

export function parseControlRequest(
  rec: Record<string, unknown>,
): ClaudeControlRequest | null {
  const type = stringField(rec, "type");
  if (type !== "control_request" && type !== "sdk_control_request") {
    return null;
  }
  const nested = asRecord(rec.request);
  const requestId =
    stringField(rec, "request_id") ?? stringField(nested, "request_id") ?? "";
  const subtype =
    stringField(nested, "subtype") ?? stringField(rec, "subtype") ?? "";
  if (!requestId || !subtype) return null;
  const input =
    asRecord(nested?.input) ??
    asRecord(nested?.tool_input) ??
    asRecord(rec.input) ??
    {};
  return {
    requestId,
    subtype,
    toolName: stringField(nested, "tool_name") ?? stringField(rec, "tool_name"),
    input,
    toolUseId:
      stringField(nested, "tool_use_id") ??
      stringField(nested, "toolUseID") ??
      stringField(rec, "tool_use_id"),
  };
}

export function parseControlCancelId(
  rec: Record<string, unknown>,
): string | undefined {
  if (stringField(rec, "type") !== "control_cancel_request") return undefined;
  return (
    stringField(rec, "request_id") ??
    stringField(asRecord(rec.request), "request_id")
  );
}

export function sessionIdFromMessage(
  rec: Record<string, unknown>,
): string | undefined {
  const type = stringField(rec, "type");
  const subtype = stringField(rec, "subtype");
  if (type === "system" && subtype?.startsWith("hook_")) return undefined;
  return stringField(rec, "session_id");
}

/**
 * Claude Code pings `system/status` for every request lifecycle step
 * ("requesting", "responding", …). Codex and opencode only emit status text for
 * notable events — retries, warnings, compaction — so drop the lifecycle chatter
 * here and keep the transcript comparable across harnesses.
 */
const LIFECYCLE_STATUSES = new Set([
  "requesting",
  "request",
  "responding",
  "response",
  "streaming",
  "thinking",
  "working",
  "running",
  "pending",
  "queued",
  "waiting",
  "in_progress",
  "tool_use",
  "idle",
  "done",
  "completed",
  "status",
  "compact",
]);

export function statusTextFromSystem(
  rec: Record<string, unknown>,
): string | undefined {
  if (stringField(rec, "type") !== "system") return undefined;
  const subtype = stringField(rec, "subtype") ?? "";
  const compact = subtype.startsWith("compact");
  if (subtype !== "status" && !compact) return undefined;
  // Prose lives in `message`; `status` carries the bare lifecycle token.
  const text = (stringField(rec, "message") ?? "").trim();
  const notable =
    text && !LIFECYCLE_STATUSES.has(text.toLowerCase().replace(/[\s.…]+$/, ""));
  if (notable) return text;
  // Compaction is worth one row even when the CLI sends no prose with it.
  return compact ? "Compacted context" : undefined;
}

export function turnStatusFromResult(rec: Record<string, unknown>): {
  status: "completed" | "failed" | "interrupted" | "cancelled";
  error?: string;
} {
  const subtype = stringField(rec, "subtype") ?? "";
  if (subtype === "success") return { status: "completed" };
  const errors = Array.isArray(rec.errors)
    ? rec.errors.filter((item): item is string => typeof item === "string")
    : [];
  const joined = errors.join(" ").toLowerCase();
  const terminal = stringField(rec, "terminal_reason") ?? "";
  if (
    terminal === "aborted_tools" ||
    terminal === "aborted_streaming" ||
    joined.includes("interrupt")
  ) {
    return { status: "interrupted" };
  }
  if (joined.includes("cancel")) return { status: "cancelled" };
  const error = errors.find((item) => !item.startsWith("[ede_diagnostic]"));
  return { status: "failed", error: error ?? "Claude turn failed." };
}

export type ClaudeTextState = {
  recordSequence: number;
  entries: ReadonlyMap<string, string>;
  tailKey: string | null;
  highestPublishedContentIndex: number;
  fallbackAllowed: boolean;
};

export type ClaudeTextStateResult = {
  state: ClaudeTextState;
  event?: Extract<HarnessEvent, { type: "message.delta" | "reasoning.delta" }>;
  conflict?: "position" | "text";
};

export function createClaudeTextState(recordSequence: number): ClaudeTextState {
  return {
    recordSequence,
    entries: new Map(),
    tailKey: null,
    highestPublishedContentIndex: -1,
    fallbackAllowed: true,
  };
}

export function invalidateClaudeTextTail(
  state: ClaudeTextState,
): ClaudeTextState {
  return { ...state, tailKey: null, fallbackAllowed: false };
}

export function markClaudeContentPublished(
  state: ClaudeTextState,
  contentIndex: number,
): ClaudeTextState {
  return {
    ...state,
    tailKey: null,
    highestPublishedContentIndex: Math.max(
      state.highestPublishedContentIndex,
      contentIndex,
    ),
  };
}

export function reconcileClaudeCompletedTool(
  state: ClaudeTextState,
  input: { contentIndex: number; alreadyPublished: boolean },
): { state: ClaudeTextState; publish: boolean } {
  if (input.alreadyPublished) return { state, publish: false };
  return {
    state: markClaudeContentPublished(state, input.contentIndex),
    publish: true,
  };
}

export function applyClaudeTextDelta(
  state: ClaudeTextState,
  delta: {
    kind: "assistant" | "reasoning";
    contentIndex: number;
    text: string;
  },
): ClaudeTextStateResult {
  const key = `${delta.kind}:${delta.contentIndex}`;
  const entries = new Map(state.entries);
  entries.set(key, (entries.get(key) ?? "") + delta.text);
  return {
    state: {
      ...state,
      entries,
      tailKey: key,
      highestPublishedContentIndex: Math.max(
        state.highestPublishedContentIndex,
        delta.contentIndex,
      ),
    },
    event: claudeTextEvent(delta.kind, delta.text),
  };
}

export function applyClaudeCompletedText(
  state: ClaudeTextState,
  part: Extract<ClaudeCompletedPart, { kind: "text" | "thinking" }>,
): ClaudeTextStateResult {
  const role = part.kind === "text" ? "assistant" : "reasoning";
  const key = `${role}:${part.contentIndex}`;
  const streamed = state.entries.get(key);
  const relationship = classifyCompletedText({
    streamed: streamed ?? "",
    completed: part.text,
  });
  if (relationship.kind === "already-emitted") return { state };
  if (relationship.kind === "conflict") {
    return { state, conflict: "text" };
  }
  const positionSafe =
    streamed === undefined
      ? state.fallbackAllowed &&
        part.contentIndex > state.highestPublishedContentIndex
      : state.tailKey === key;
  if (!positionSafe) return { state, conflict: "position" };

  const text =
    relationship.kind === "fallback" ? relationship.text : relationship.suffix;
  const entries = new Map(state.entries);
  entries.set(key, part.text);
  return {
    state: {
      ...state,
      entries,
      tailKey: key,
      highestPublishedContentIndex: Math.max(
        state.highestPublishedContentIndex,
        part.contentIndex,
      ),
    },
    ...(text ? { event: claudeTextEvent(role, text) } : {}),
  };
}

function claudeTextEvent(
  role: "assistant" | "reasoning",
  text: string,
): Extract<HarnessEvent, { type: "message.delta" | "reasoning.delta" }> {
  return role === "assistant"
    ? { type: "message.delta", text }
    : { type: "reasoning.delta", text };
}

export function streamDeltaFromEvent(rec: Record<string, unknown>): {
  kind: "assistant" | "reasoning";
  contentIndex: number;
  text: string;
} | null {
  const event = asRecord(rec.event);
  if (!event || stringField(event, "type") !== "content_block_delta") {
    return null;
  }
  const contentIndex = typeof event.index === "number" ? event.index : -1;
  const delta = asRecord(event.delta);
  const deltaType = stringField(delta, "type") ?? "";
  if (deltaType === "text_delta") {
    const text = typeof delta?.text === "string" ? delta.text : "";
    return text ? { kind: "assistant", contentIndex, text } : null;
  }
  if (deltaType === "thinking_delta") {
    const text = typeof delta?.thinking === "string" ? delta.thinking : "";
    return text ? { kind: "reasoning", contentIndex, text } : null;
  }
  return null;
}

export function toolStartFromEvent(rec: Record<string, unknown>): {
  index: number;
  id: string;
  name: string;
  input: Record<string, unknown>;
} | null {
  const event = asRecord(rec.event);
  if (!event || stringField(event, "type") !== "content_block_start") {
    return null;
  }
  const block = asRecord(event.content_block);
  if (!block) return null;
  const blockType = stringField(block, "type") ?? "";
  if (
    blockType !== "tool_use" &&
    blockType !== "server_tool_use" &&
    blockType !== "mcp_tool_use"
  ) {
    return null;
  }
  const id = stringField(block, "id");
  const name = stringField(block, "name");
  if (!id || !name) return null;
  const index = typeof event.index === "number" ? event.index : -1;
  return {
    index,
    id,
    name,
    input: asRecord(block.input) ?? {},
  };
}

export function inputJsonDeltaFromEvent(
  rec: Record<string, unknown>,
): { index: number; partial: string } | null {
  const event = asRecord(rec.event);
  if (!event || stringField(event, "type") !== "content_block_delta") {
    return null;
  }
  const delta = asRecord(event.delta);
  if (stringField(delta, "type") !== "input_json_delta") return null;
  const partial =
    typeof delta?.partial_json === "string" ? delta.partial_json : "";
  if (!partial) return null;
  const index = typeof event.index === "number" ? event.index : -1;
  return { index, partial };
}

export function isSubagentMessage(rec: Record<string, unknown>): boolean {
  const parent = rec.parent_tool_use_id;
  return typeof parent === "string" && parent.length > 0;
}

export type ClaudeToolUse = {
  id: string;
  name: string;
  input: Record<string, unknown>;
};

export type ClaudeCompletedPart =
  | { kind: "text"; contentIndex: number; text: string }
  | { kind: "thinking"; contentIndex: number; text: string }
  | { kind: "tool"; contentIndex: number; use: ClaudeToolUse };

export function completedAssistantParts(
  rec: Record<string, unknown>,
): ClaudeCompletedPart[] {
  const content = asRecord(rec.message)?.content;
  if (!Array.isArray(content)) return [];
  return content.flatMap((block, contentIndex): ClaudeCompletedPart[] => {
    const row = asRecord(block);
    const type = stringField(row, "type");
    if (type === "text") {
      const text = typeof row?.text === "string" ? row.text : "";
      return text ? [{ kind: "text", contentIndex, text }] : [];
    }
    if (type === "thinking") {
      const text = typeof row?.thinking === "string" ? row.thinking : "";
      return text ? [{ kind: "thinking", contentIndex, text }] : [];
    }
    if (type !== "tool_use") return [];
    const id = stringField(row, "id");
    const name = stringField(row, "name");
    if (!id || !name) return [];
    return [
      {
        kind: "tool",
        contentIndex,
        use: { id, name, input: asRecord(row?.input) ?? {} },
      },
    ];
  });
}

export function assistantTextBlocks(rec: Record<string, unknown>): string[] {
  const message = asRecord(rec.message);
  const content = message?.content;
  if (!Array.isArray(content)) return [];
  return content.flatMap((block) => {
    const row = asRecord(block);
    if (stringField(row, "type") !== "text") return [];
    const text = typeof row?.text === "string" ? row.text : "";
    return text ? [text] : [];
  });
}

export function assistantToolUses(
  rec: Record<string, unknown>,
): ClaudeToolUse[] {
  const message = asRecord(rec.message);
  const content = message?.content;
  if (!Array.isArray(content)) return [];
  return content.flatMap((block) => {
    const row = asRecord(block);
    if (!row || stringField(row, "type") !== "tool_use") return [];
    const id = stringField(row, "id");
    const name = stringField(row, "name");
    if (!id || !name) return [];
    return [{ id, name, input: asRecord(row.input) ?? {} }];
  });
}

export function toolResultsFromUserMessage(
  rec: Record<string, unknown>,
): Array<{
  toolUseId: string;
  isError: boolean;
  text: string;
}> {
  const message = asRecord(rec.message);
  const content = message?.content;
  if (!Array.isArray(content)) return [];
  return content.flatMap((block) => {
    const row = asRecord(block);
    if (!row || stringField(row, "type") !== "tool_result") return [];
    const toolUseId = stringField(row, "tool_use_id");
    if (!toolUseId) return [];
    const text = toolResultText(row.content);
    return [
      {
        toolUseId,
        isError: row.is_error === true,
        text,
      },
    ];
  });
}

function toolResultText(value: unknown): string {
  if (typeof value === "string") return value;
  if (!Array.isArray(value)) return "";
  return value
    .flatMap((block) => {
      if (typeof block === "string") return [block];
      const row = asRecord(block);
      if (
        stringField(row, "type") === "text" &&
        typeof row?.text === "string"
      ) {
        return [row.text];
      }
      return [];
    })
    .join("");
}

export function extractExitPlanModePlan(value: unknown): string | undefined {
  const rec = asRecord(value);
  const plan = stringField(rec, "plan");
  return plan;
}

export function extractAskUserQuestionTitle(
  input: Record<string, unknown>,
): string {
  const questions = Array.isArray(input.questions) ? input.questions : [];
  const first = asRecord(questions[0]);
  return (
    stringField(first, "header") ??
    stringField(first, "question") ??
    "Claude question"
  );
}

export function askUserQuestionAllowInput(
  input: Record<string, unknown>,
): Record<string, unknown> {
  const questions = Array.isArray(input.questions) ? input.questions : [];
  const answers: Record<string, string> = {};
  for (const question of questions) {
    const rec = asRecord(question);
    const prompt =
      stringField(rec, "question") ?? stringField(rec, "header") ?? "";
    const options = Array.isArray(rec?.options) ? rec.options : [];
    const first = asRecord(options[0]);
    const label = stringField(first, "label");
    if (prompt && label) answers[prompt] = label;
  }
  return { questions: input.questions, answers };
}

export function tryParseJsonRecord(
  value: string,
): Record<string, unknown> | undefined {
  try {
    return asRecord(JSON.parse(value)) ?? undefined;
  } catch {
    return undefined;
  }
}

export function planTextFromTodos(
  input: Record<string, unknown>,
): string | null {
  const todos = input.todos;
  if (!Array.isArray(todos) || todos.length === 0) return null;
  const lines = todos.flatMap((todo) => {
    const rec = asRecord(todo);
    const step =
      stringField(rec, "content") ?? stringField(rec, "activeForm") ?? "Task";
    const status = stringField(rec, "status") ?? "pending";
    const mark =
      status === "completed" ? "[x]" : status === "in_progress" ? "[~]" : "[ ]";
    return [`${mark} ${step}`];
  });
  return lines.length > 0 ? lines.join("\n") : null;
}

export function isTodoTool(toolName: string): boolean {
  return toolName.toLowerCase().includes("todowrite");
}

export function toolKindFromName(toolName: string): string {
  const normalized = toolName.toLowerCase();
  if (
    normalized.includes("bash") ||
    normalized.includes("command") ||
    normalized.includes("shell") ||
    normalized.includes("terminal")
  ) {
    return "execute";
  }
  if (
    normalized.includes("edit") ||
    normalized.includes("write") ||
    normalized.includes("patch") ||
    normalized.includes("replace") ||
    normalized.includes("multiedit")
  ) {
    return "edit";
  }
  if (normalized === "read" || normalized.includes("read")) return "read";
  if (
    normalized.includes("grep") ||
    normalized.includes("glob") ||
    normalized.includes("search") ||
    normalized.includes("websearch")
  ) {
    return "search";
  }
  return toolName;
}

export function toolTitle(
  name: string,
  input: Record<string, unknown>,
): string {
  const kind = toolKindFromName(name);
  const preview = extractToolPreview(
    { title: name, name, kind, rawInput: input, input },
    { title: name, name, kind, rawInput: input },
  );
  return (
    composeToolTitle({
      kind,
      title: name,
      path: preview?.path,
      query: preview?.query,
      previewKind: preview?.kind,
    }) || name
  );
}

export function previewFromTool(
  name: string,
  input: Record<string, unknown>,
  output?: string,
): ToolPreview | undefined {
  const kind = toolKindFromName(name);
  return extractToolPreview(
    {
      title: name,
      name,
      kind,
      input,
      rawInput: input,
      content: output,
    },
    {
      title: name,
      name,
      kind,
      rawInput: input,
    },
  );
}

export function summarizeToolRequest(
  toolName: string,
  input: Record<string, unknown>,
): string {
  const command = stringField(input, "command") ?? stringField(input, "cmd");
  if (command) return `${toolName}: ${command.slice(0, 400)}`;
  const description = stringField(input, "description");
  if (description) return description;
  try {
    const serialized = JSON.stringify(input);
    if (serialized.length <= 400) return `${toolName}: ${serialized}`;
    return `${toolName}: ${serialized.slice(0, 397)}...`;
  } catch {
    return toolName;
  }
}

export function claudeSettingsKey(input: {
  model: string;
  effort?: string;
  fast?: string;
  thinking?: string;
  context?: string;
  runtimeMode: RuntimeMode;
}): string {
  return [
    input.model,
    input.effort ?? "",
    input.fast ?? "",
    input.thinking ?? "",
    input.context ?? "",
    input.runtimeMode,
  ].join("|");
}

function numberField(
  rec: Record<string, unknown> | null | undefined,
  key: string,
): number {
  const value = rec?.[key];
  return typeof value === "number" && Number.isFinite(value) ? value : 0;
}

/**
 * Tokens occupying the window for one request.
 *
 * Cached reads still take up window space, so they count the same as fresh
 * input; output counts because it carries into the next request.
 */
function contextUsedFromUsage(usage: Record<string, unknown> | null): number {
  if (!usage) return 0;
  return (
    numberField(usage, "input_tokens") +
    numberField(usage, "cache_creation_input_tokens") +
    numberField(usage, "cache_read_input_tokens") +
    numberField(usage, "output_tokens")
  );
}

/**
 * Context level from an `assistant` message. Callers must skip subagent
 * messages — subagents run their own window and would make the reading jump.
 */
export function contextUsedFromAssistant(
  rec: Record<string, unknown>,
): number | undefined {
  const usage = asRecord(asRecord(rec.message)?.usage);
  if (!usage) return undefined;
  const used = contextUsedFromUsage(usage);
  return used > 0 ? used : undefined;
}

/**
 * Context level and window from a turn `result`.
 *
 * `usage` at the top level sums every iteration of the turn, so the last entry
 * of `usage.iterations` is what actually sits in the window. `modelUsage`
 * carries the window itself, which is why we let the CLI tell us rather than
 * keeping a model table in sync.
 */
export function contextFromResult(
  rec: Record<string, unknown>,
): { used?: number; window?: number } | undefined {
  const usage = asRecord(rec.usage);
  const iterations = Array.isArray(usage?.iterations) ? usage.iterations : [];
  const last = asRecord(iterations[iterations.length - 1]);
  const used = contextUsedFromUsage(last ?? usage);

  let window: number | undefined;
  const modelUsage = asRecord(rec.modelUsage);
  for (const entry of Object.values(modelUsage ?? {})) {
    const contextWindow = numberField(asRecord(entry), "contextWindow");
    if (contextWindow > 0) {
      window = Math.max(window ?? 0, contextWindow);
    }
  }

  if (!used && !window) return undefined;
  return { used: used > 0 ? used : undefined, window };
}
