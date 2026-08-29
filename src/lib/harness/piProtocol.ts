import type { Attachment, ToolPreview } from "../session";
import type { AgentModel, ModelSetting } from "../models";
import type { PiFlavor } from "./piFlavor";
import { composeToolTitle, extractToolPreview } from "./preview";
import { streamTextDelta } from "./streamText";

/** Images Pi RPC accepts on `prompt` / `steer`. */
export const SUPPORTED_PI_IMAGE_MIME_TYPES = new Set([
  "image/gif",
  "image/jpeg",
  "image/png",
  "image/webp",
]);

export const PI_THINKING_LEVELS = [
  "off",
  "minimal",
  "low",
  "medium",
  "high",
  "xhigh",
  "max",
] as const;

export type PiThinkingLevel = (typeof PI_THINKING_LEVELS)[number];

export type PiModelRef = {
  provider: string;
  modelId: string;
};

export type PiImage = {
  type: "image";
  data: string;
  mimeType: string;
};

export type PiExtensionUiRequest =
  | {
      id: string;
      method: "select";
      title: string;
      options: string[];
    }
  | {
      id: string;
      method: "confirm";
      title: string;
      message: string;
    }
  | {
      id: string;
      method: "input" | "editor";
      title: string;
    }
  | {
      id: string;
      method: "notify" | "setStatus" | "setWidget" | "setTitle" | "set_editor_text";
      title?: string;
    };

export type PiRpcResponse = {
  id?: string;
  command: string;
  success: boolean;
  error?: string;
  data?: unknown;
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

export function parseJsonLine(line: string): Record<string, unknown> | null {
  const trimmed = line.trim();
  if (!trimmed.startsWith("{")) return null;
  try {
    return asRecord(JSON.parse(trimmed));
  } catch {
    return null;
  }
}

export function parsePiVersion(output: string): string | null {
  const match = output.match(/\d+\.\d+\.\d+/);
  return match?.[0] ?? null;
}

/**
 * Spawn args for a live session. Intentionally omits `--no-extensions` so the
 * user's global Pi packages (todos, subagents, custom tools) still load.
 * Project-local `.pi` resources follow Pi's saved trust.json; RPC never prompts.
 */
export function buildPiSpawnArgs(
  flavor: PiFlavor,
  input: {
    model?: string;
    resume?: string;
    /** Catalog probes and isolated jobs: do not write a session file. */
    noSession?: boolean;
    /** Catalog probes and throwaway text jobs — never for live chat. */
    noExtensions?: boolean;
    /** Titles and other one-shot prompts: no tools, skills, or project context. */
    isolated?: boolean;
  },
): string[] {
  const args = ["--mode", "rpc"];
  if (input.isolated || input.noSession) args.push("--no-session");
  if (input.isolated || input.noExtensions) args.push("--no-extensions");
  if (input.isolated) {
    args.push(...flavor.isolateFlags);
  }
  if (input.resume?.trim()) {
    args.push(flavor.resumeFlag, input.resume.trim());
  }
  const model = input.model?.trim();
  if (model) args.push("--model", model);
  return args;
}

export function parsePiModelRef(nativeId: string | undefined): PiModelRef | null {
  if (!nativeId) return null;
  const trimmed = nativeId.trim();
  if (!trimmed) return null;
  const separator = trimmed.indexOf("/");
  if (separator <= 0 || separator === trimmed.length - 1) return null;
  return {
    provider: trimmed.slice(0, separator),
    modelId: trimmed.slice(separator + 1),
  };
}

export function piNativeId(provider: string, modelId: string): string {
  return `${provider}/${modelId}`;
}

export function toPiImages(attachments: Attachment[] | undefined): PiImage[] {
  const images: PiImage[] = [];
  for (const attachment of attachments ?? []) {
    if (attachment.kind !== "image" || !attachment.data) continue;
    const mimeType = attachment.mimeType.trim().toLowerCase();
    if (!SUPPORTED_PI_IMAGE_MIME_TYPES.has(mimeType)) continue;
    images.push({
      type: "image",
      data: attachment.data,
      mimeType,
    });
  }
  return images;
}

export function buildPiPrompt(input: {
  text: string;
  attachments?: Attachment[];
  streaming?: boolean;
}): Record<string, unknown> {
  const command: Record<string, unknown> = {
    type: "prompt",
    message: input.text,
  };
  const images = toPiImages(input.attachments);
  if (images.length > 0) command.images = images;
  if (input.streaming) command.streamingBehavior = "steer";
  return command;
}

export function buildPiSteer(input: {
  text: string;
  attachments?: Attachment[];
}): Record<string, unknown> {
  const command: Record<string, unknown> = {
    type: "steer",
    message: input.text,
  };
  const images = toPiImages(input.attachments);
  if (images.length > 0) command.images = images;
  return command;
}

export function parseRpcResponse(rec: Record<string, unknown>): PiRpcResponse | null {
  if (stringField(rec, "type") !== "response") return null;
  const command = stringField(rec, "command") ?? "unknown";
  const id = stringField(rec, "id");
  const success = rec.success === true;
  const error = stringField(rec, "error");
  return {
    ...(id ? { id } : {}),
    command,
    success,
    ...(error ? { error } : {}),
    data: rec.data,
  };
}

export function parseExtensionUiRequest(
  rec: Record<string, unknown>,
): PiExtensionUiRequest | null {
  if (stringField(rec, "type") !== "extension_ui_request") return null;
  const id = stringField(rec, "id");
  const method = stringField(rec, "method");
  if (!id || !method) return null;
  if (method === "select") {
    const options = Array.isArray(rec.options)
      ? rec.options.filter((item): item is string => typeof item === "string")
      : [];
    return {
      id,
      method,
      title: stringField(rec, "title") ?? "Choose an option",
      options,
    };
  }
  if (method === "confirm") {
    return {
      id,
      method,
      title: stringField(rec, "title") ?? "Confirm",
      message: stringField(rec, "message") ?? "",
    };
  }
  if (method === "input" || method === "editor") {
    return { id, method, title: stringField(rec, "title") ?? method };
  }
  if (
    method === "notify" ||
    method === "setStatus" ||
    method === "setWidget" ||
    method === "setTitle" ||
    method === "set_editor_text"
  ) {
    return {
      id,
      method,
      title:
        stringField(rec, "message") ??
        stringField(rec, "statusText") ??
        stringField(rec, "title") ??
        stringField(rec, "text"),
    };
  }
  return null;
}

export function extensionUiResponse(
  request: PiExtensionUiRequest,
  decision: "allow" | "deny",
): Record<string, unknown> {
  if (decision === "deny") {
    return { type: "extension_ui_response", id: request.id, cancelled: true };
  }
  if (request.method === "confirm") {
    return { type: "extension_ui_response", id: request.id, confirmed: true };
  }
  if (request.method === "select") {
    const value = request.options[0] ?? "";
    return { type: "extension_ui_response", id: request.id, value };
  }
  return { type: "extension_ui_response", id: request.id, cancelled: true };
}

export function extensionUiTitle(request: PiExtensionUiRequest): string {
  if (request.method === "confirm") {
    return [request.title, request.message].filter(Boolean).join(" — ");
  }
  if (request.method === "select") {
    return request.title;
  }
  return request.title ?? "Pi extension";
}

export function needsExtensionUiReply(request: PiExtensionUiRequest): boolean {
  return (
    request.method === "confirm" ||
    request.method === "select" ||
    request.method === "input" ||
    request.method === "editor"
  );
}

export function sessionFromState(data: unknown): {
  sessionId?: string;
  sessionFile?: string;
  contextWindow?: number;
} {
  const rec = asRecord(data);
  const model = asRecord(rec?.model);
  const window = numberField(model, "contextWindow");
  return {
    sessionId: stringField(rec, "sessionId"),
    sessionFile: stringField(rec, "sessionFile"),
    ...(window && window > 0 ? { contextWindow: window } : {}),
  };
}

/**
 * Session-store ids may only be ASCII letters, digits, `-`, and `_`.
 * Pi's `sessionFile` is a filesystem path, so persist would reject the whole
 * upsert (and the sidebar would never get a git snapshot). `--session` accepts
 * the UUID `sessionId` as well, so that's what we bind.
 */
export function providerSessionIdFromState(data: unknown): string | undefined {
  const sessionId = sessionFromState(data).sessionId?.trim();
  if (!sessionId || !/^[A-Za-z0-9_-]+$/.test(sessionId)) return undefined;
  return sessionId;
}

/**
 * Current Pi puts streaming usage on the frame. 0.80.x put a finished
 * assistant total on `message` and a live total on `assistantMessageEvent.partial`.
 * Tool-result messages can carry nested LLM usage for a sub-call; that is not
 * the context-window level, so only assistant `message.usage` counts.
 */
export function contextFromUsage(
  rec: Record<string, unknown>,
  window?: number,
): { used?: number; window?: number } | null {
  const usage =
    asRecord(rec.usage) ??
    assistantMessageUsage(rec) ??
    asRecord(asRecord(asRecord(rec.assistantMessageEvent)?.partial)?.usage);
  if (!usage) return null;
  const used =
    numberField(usage, "totalTokens") ||
    (numberField(usage, "input") ?? 0) +
      (numberField(usage, "output") ?? 0) +
      (numberField(usage, "cacheRead") ?? 0) +
      (numberField(usage, "cacheWrite") ?? 0);
  if (!used) return window && window > 0 ? { window } : null;
  return window && window > 0 ? { used, window } : { used };
}

export function contextFromSessionStats(data: unknown): {
  used?: number;
  window?: number;
} | null {
  const rec = asRecord(data);
  const usage = asRecord(rec?.contextUsage);
  if (!usage) return null;
  const used = numberField(usage, "tokens");
  const window = numberField(usage, "contextWindow");
  if (!used && !window) return null;
  return {
    ...(used ? { used } : {}),
    ...(window && window > 0 ? { window } : {}),
  };
}

export function assistantDeltaFromEvent(
  rec: Record<string, unknown>,
): { kind: "text" | "thinking"; text: string } | null {
  if (stringField(rec, "type") !== "message_update") return null;
  const event = asRecord(rec.assistantMessageEvent);
  const type = stringField(event, "type");
  const delta = streamTextDelta(event?.delta);
  if (!delta) return null;
  if (type === "text_delta") return { kind: "text", text: delta };
  if (type === "thinking_delta") return { kind: "thinking", text: delta };
  return null;
}

export function toolCallStartFromEvent(
  rec: Record<string, unknown>,
): { id: string; name: string; index: number } | null {
  if (stringField(rec, "type") !== "message_update") return null;
  const event = asRecord(rec.assistantMessageEvent);
  if (stringField(event, "type") !== "toolcall_start") return null;
  const id = stringField(event, "id");
  const name = stringField(event, "toolName") ?? stringField(event, "name");
  if (!id || !name) return null;
  const index = numberField(event, "contentIndex") ?? -1;
  return { id, name, index };
}

export function toolCallDeltaFromEvent(
  rec: Record<string, unknown>,
): { index: number; delta: string } | null {
  if (stringField(rec, "type") !== "message_update") return null;
  const event = asRecord(rec.assistantMessageEvent);
  if (stringField(event, "type") !== "toolcall_delta") return null;
  const delta = streamTextDelta(event?.delta);
  if (!delta) return null;
  return {
    index: numberField(event, "contentIndex") ?? -1,
    delta,
  };
}

export function toolCallEndFromEvent(
  rec: Record<string, unknown>,
): { id: string; name: string; input: Record<string, unknown> } | null {
  if (stringField(rec, "type") !== "message_update") return null;
  const event = asRecord(rec.assistantMessageEvent);
  if (stringField(event, "type") !== "toolcall_end") return null;
  const call = asRecord(event?.toolCall) ?? event;
  const id = stringField(call, "id") ?? stringField(event, "id");
  const name =
    stringField(call, "name") ??
    stringField(call, "toolName") ??
    stringField(event, "toolName");
  if (!id || !name) return null;
  const input =
    asRecord(call?.arguments) ??
    asRecord(call?.args) ??
    asRecord(event?.arguments) ??
    {};
  return { id, name, input };
}

export function toolExecutionStartFromEvent(
  rec: Record<string, unknown>,
): { id: string; name: string; input: Record<string, unknown> } | null {
  if (stringField(rec, "type") !== "tool_execution_start") return null;
  const id = stringField(rec, "toolCallId");
  const name = stringField(rec, "toolName") ?? "tool";
  if (!id) return null;
  return { id, name, input: asRecord(rec.args) ?? {} };
}

export function toolExecutionUpdateFromEvent(
  rec: Record<string, unknown>,
): { id: string; name?: string; detail?: string } | null {
  if (stringField(rec, "type") !== "tool_execution_update") return null;
  const id = stringField(rec, "toolCallId");
  if (!id) return null;
  const partial = asRecord(rec.partialResult);
  return {
    id,
    name: stringField(rec, "toolName"),
    detail: textFromContent(partial?.content) || undefined,
  };
}

export function toolExecutionEndFromEvent(
  rec: Record<string, unknown>,
): {
  id: string;
  name?: string;
  detail?: string;
  isError: boolean;
} | null {
  if (stringField(rec, "type") !== "tool_execution_end") return null;
  const id = stringField(rec, "toolCallId");
  if (!id) return null;
  const result = asRecord(rec.result);
  return {
    id,
    name: stringField(rec, "toolName"),
    detail: textFromContent(result?.content) || undefined,
    isError: rec.isError === true,
  };
}

/**
 * A failed turn is reported inside the assistant message, not as an error
 * frame: `stopReason: "error"` with the reason in `errorMessage`. Empty string
 * means "failed, no reason".
 */
export function turnErrorFromEvent(rec: Record<string, unknown>): string | null {
  if (stringField(rec, "type") !== "message_end") return null;
  const message = asRecord(rec.message);
  if (stringField(message, "role") !== "assistant") return null;
  if (stringField(message, "stopReason") !== "error") return null;
  return stringField(message, "errorMessage") ?? "";
}

export function isAgentSettled(rec: Record<string, unknown>): boolean {
  return stringField(rec, "type") === "agent_settled";
}

export function agentEndWillRetry(rec: Record<string, unknown>): boolean | null {
  if (stringField(rec, "type") !== "agent_end") return null;
  return rec.willRetry === true;
}

export function statusFromPiEvent(rec: Record<string, unknown>): string | null {
  const type = stringField(rec, "type");
  if (type === "compaction_start") return "Compacting context…";
  if (type === "auto_retry_start") {
    const attempt = numberField(rec, "attempt");
    const max = numberField(rec, "maxAttempts");
    if (attempt && max) return `Retrying (${attempt}/${max})…`;
    return "Retrying…";
  }
  if (type === "extension_error") {
    return stringField(rec, "error") ?? "Pi extension error";
  }
  return null;
}

export function tryParseJsonRecord(partial: string): Record<string, unknown> | null {
  try {
    return asRecord(JSON.parse(partial));
  } catch {
    return null;
  }
}

export function textFromContent(content: unknown): string {
  if (typeof content === "string") return content;
  if (!Array.isArray(content)) return "";
  const parts: string[] = [];
  for (const item of content) {
    if (typeof item === "string") {
      parts.push(item);
      continue;
    }
    const rec = asRecord(item);
    const text = stringField(rec, "text");
    if (text) parts.push(text);
  }
  return parts.join("");
}

export function toolKindFromName(toolName: string): string {
  const normalized = toolName.toLowerCase();
  if (
    normalized.includes("bash") ||
    normalized.includes("command") ||
    normalized.includes("shell")
  ) {
    return "execute";
  }
  if (
    normalized.includes("edit") ||
    normalized.includes("write") ||
    normalized.includes("patch") ||
    normalized.includes("replace")
  ) {
    return "edit";
  }
  if (normalized === "read" || normalized.includes("read")) return "read";
  if (
    normalized.includes("grep") ||
    normalized.includes("glob") ||
    normalized.includes("search") ||
    normalized.includes("find") ||
    normalized === "ls"
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
  const path =
    stringField(input, "path") ??
    stringField(input, "file_path") ??
    stringField(input, "filePath");
  if (path) return `${toolName}: ${path}`;
  try {
    const serialized = JSON.stringify(input);
    if (serialized.length <= 400) return `${toolName}: ${serialized}`;
    return `${toolName}: ${serialized.slice(0, 397)}...`;
  } catch {
    return toolName;
  }
}

export function modelsFromRpcData(
  flavor: PiFlavor,
  data: unknown,
): AgentModel[] {
  const rec = asRecord(data);
  const list = Array.isArray(rec?.models)
    ? rec.models
    : Array.isArray(data)
      ? data
      : [];
  const models: AgentModel[] = [];
  const seen = new Set<string>();
  for (const item of list) {
    const model = asRecord(item);
    if (!model) continue;
    const modelId = stringField(model, "id");
    const provider = stringField(model, "provider");
    if (!modelId || !provider) continue;
    const nativeId = piNativeId(provider, modelId);
    if (seen.has(nativeId)) continue;
    seen.add(nativeId);
    const name = stringField(model, "name") || modelId;
    const contextWindow = numberField(model, "contextWindow");
    const settings = thinkingSetting(model.reasoning === true);
    models.push({
      id: `${flavor.id}:${nativeId}`,
      harness: flavor.id,
      name,
      nativeId,
      ...(settings ? { settings: [settings] } : {}),
      ...(contextWindow && contextWindow > 0 ? { contextWindow } : {}),
    });
  }
  return models.sort((left, right) => left.name.localeCompare(right.name));
}

export function thinkingSetting(reasoning: boolean): ModelSetting | undefined {
  if (!reasoning) return undefined;
  return {
    id: "thinking",
    label: "Thinking",
    kind: "select",
    value: "medium",
    options: PI_THINKING_LEVELS.map((value) => ({
      value,
      label: thinkingLabel(value),
    })),
  };
}

export function isPiThinkingLevel(value: string | undefined): value is PiThinkingLevel {
  return (
    !!value && (PI_THINKING_LEVELS as readonly string[]).includes(value)
  );
}

function thinkingLabel(level: PiThinkingLevel): string {
  if (level === "xhigh") return "Extra High";
  if (level === "off") return "Off";
  return level.slice(0, 1).toUpperCase() + level.slice(1);
}

function assistantMessageUsage(
  rec: Record<string, unknown>,
): Record<string, unknown> | null {
  const message = asRecord(rec.message);
  if (stringField(message, "role") !== "assistant") return null;
  return asRecord(message?.usage);
}

function numberField(
  rec: Record<string, unknown> | null | undefined,
  key: string,
): number | undefined {
  const value = rec?.[key];
  return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}
