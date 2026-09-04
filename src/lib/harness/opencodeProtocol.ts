import type { Attachment, RuntimeMode, ToolPreview } from "../session";
import { isTaskListToolName } from "../taskList";
import { extractToolPreview } from "./preview";
import type { HarnessEvent } from "./types";

export const MINIMUM_OPENCODE_VERSION = "1.14.19";
export const OPENCODE_SERVER_READY_PREFIX = "opencode server listening";
export const KNOWN_HIDDEN_AGENTS = new Set(["compaction", "summary", "title"]);

const OPENCODE_DEFAULT_TITLE_PATTERN =
  /^(New session - |Child session - )\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/;

export type ParsedOpenCodeModelSlug = {
  providerID: string;
  modelID: string;
};

export type OpenCodePermissionRule = {
  permission: string;
  pattern: string;
  action: "allow" | "deny" | "ask";
};

export type OpenCodePart = {
  id: string;
  type: string;
  messageID?: string;
  callID?: string;
  tool?: string;
  text?: string;
  time?: { start?: number; end?: number };
  state?: Record<string, unknown>;
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

export function parseOpenCodeModelSlug(
  slug: string | null | undefined,
): ParsedOpenCodeModelSlug | null {
  if (typeof slug !== "string") return null;
  const trimmed = slug.trim();
  const separator = trimmed.indexOf("/");
  if (separator <= 0 || separator === trimmed.length - 1) return null;
  return {
    providerID: trimmed.slice(0, separator),
    modelID: trimmed.slice(separator + 1),
  };
}

export function parseServerUrlFromOutput(output: string): string | null {
  for (const line of output.split("\n")) {
    const trimmed = line.trim();
    if (!trimmed.toLowerCase().includes("listening")) continue;
    const match = trimmed.match(/on\s+(https?:\/\/[^\s]+)/i);
    if (match?.[1]) return match[1].replace(/[.,;]+$/, "");
    if (trimmed.startsWith(OPENCODE_SERVER_READY_PREFIX)) {
      const fallback = trimmed.match(/(https?:\/\/[^\s]+)/i);
      if (fallback?.[1]) return fallback[1].replace(/[.,;]+$/, "");
    }
  }
  return null;
}

export function parseOpenCodeVersion(output: string): string | null {
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

export function isOpenCodeDefaultTitle(title: string): boolean {
  return OPENCODE_DEFAULT_TITLE_PATTERN.test(title);
}

export function isOpenCodeNotFound(cause: unknown): boolean {
  const seen = new Set<object>();
  const queue: unknown[] = [cause];
  for (let steps = 0; queue.length > 0 && steps < 32; steps += 1) {
    const node = queue.shift();
    if (node === null || typeof node !== "object" || seen.has(node)) continue;
    seen.add(node);
    const record = node as Record<string, unknown>;
    const response = record.response;
    const statuses = [
      record.status,
      record.statusCode,
      response !== null && typeof response === "object"
        ? (response as { status?: unknown }).status
        : undefined,
    ].filter((status): status is number => typeof status === "number");
    if (statuses.includes(404)) return true;
    if (statuses.length > 0) continue;
    const name = record.name;
    if (typeof name === "string" && name.toLowerCase() === "notfounderror") {
      return true;
    }
    for (const key of ["cause", "body", "error", "data"] as const) {
      if (record[key] !== undefined) queue.push(record[key]);
    }
  }
  return false;
}

export function buildOpenCodePermissionRules(
  runtimeMode: RuntimeMode,
): OpenCodePermissionRule[] {
  if (runtimeMode === "full-access") {
    return [{ permission: "*", pattern: "*", action: "allow" }];
  }
  const rules: OpenCodePermissionRule[] = [
    { permission: "*", pattern: "*", action: "ask" },
    { permission: "question", pattern: "*", action: "allow" },
  ];
  if (runtimeMode === "auto-accept-edits" || runtimeMode === "auto") {
    rules.push({ permission: "edit", pattern: "*", action: "allow" });
  }
  if (runtimeMode === "auto") {
    rules.push({ permission: "read", pattern: "*", action: "allow" });
  }
  return rules;
}

export function toOpenCodePermissionReply(
  decision: "allow" | "deny",
): "once" | "reject" {
  return decision === "allow" ? "once" : "reject";
}

export function toFileUrl(path: string): string {
  const normalized = path.replace(/\\/g, "/");
  const abs = normalized.startsWith("/") ? normalized : `/${normalized}`;
  return `file://${abs.split("/").map(encodeURIComponent).join("/")}`;
}

export function toOpenCodeFileParts(
  attachments: Attachment[] | undefined,
): Array<{ type: "file"; mime: string; filename: string; url: string }> {
  const parts: Array<{
    type: "file";
    mime: string;
    filename: string;
    url: string;
  }> = [];
  for (const attachment of attachments ?? []) {
    const url = attachment.path
      ? toFileUrl(attachment.path)
      : attachment.data
        ? `data:${attachment.mimeType};base64,${attachment.data}`
        : null;
    if (!url) continue;
    parts.push({
      type: "file",
      mime: attachment.mimeType,
      filename: attachment.name,
      url,
    });
  }
  return parts;
}

export function mergeOpenCodeAssistantText(
  previousText: string | undefined,
  nextText: string,
): { latestText: string; deltaToEmit: string } {
  const latestText =
    previousText &&
    previousText.length > nextText.length &&
    previousText.startsWith(nextText)
      ? previousText
      : nextText;
  return {
    latestText,
    deltaToEmit: latestText.slice(commonPrefixLength(previousText ?? "", latestText)),
  };
}

export function appendOpenCodeAssistantTextDelta(
  previousText: string,
  delta: string,
): { nextText: string; deltaToEmit: string } {
  return { nextText: previousText + delta, deltaToEmit: delta };
}

function commonPrefixLength(left: string, right: string): number {
  let index = 0;
  while (index < left.length && index < right.length && left[index] === right[index]) {
    index += 1;
  }
  return index;
}

export function titleCaseSlug(value: string): string {
  const segments: string[] = [];
  for (const segment of value.split(/[-_/]+/)) {
    if (segment.length > 0) {
      segments.push(segment.charAt(0).toUpperCase() + segment.slice(1));
    }
  }
  return segments.join(" ");
}

export function inferDefaultVariant(
  providerID: string,
  variants: string[],
): string | undefined {
  if (variants.length === 1) return variants[0];
  if (providerID === "anthropic" || providerID.startsWith("google")) {
    return variants.includes("high") ? "high" : undefined;
  }
  if (providerID === "openai" || providerID === "opencode") {
    return variants.includes("medium")
      ? "medium"
      : variants.includes("high")
        ? "high"
        : undefined;
  }
  return undefined;
}

export function inferDefaultAgent(agents: Array<{ name: string }>): string | undefined {
  return agents.find((agent) => agent.name === "build")?.name ?? agents[0]?.name;
}

export function toolKindFromName(toolName: string): string {
  const normalized = toolName.toLowerCase();
  if (isTaskListToolName(toolName)) return "tasks";
  if (normalized.includes("bash") || normalized.includes("command") || normalized.includes("shell")) {
    return "shell";
  }
  if (
    normalized.includes("edit") ||
    normalized.includes("write") ||
    normalized.includes("patch") ||
    normalized.includes("multiedit")
  ) {
    return "edit";
  }
  if (normalized.includes("read")) return "read";
  if (
    normalized.includes("grep") ||
    normalized.includes("glob") ||
    normalized.includes("search") ||
    normalized.includes("find")
  ) {
    return "search";
  }
  if (normalized === "skill" || normalized === "skills") return "skill";
  if (
    normalized === "agent" ||
    normalized === "task" ||
    normalized === "subagent"
  ) {
    return "agent";
  }
  return toolName;
}

export function previewFromToolPart(part: OpenCodePart): ToolPreview | undefined {
  const tool = part.tool ?? "tool";
  const state = part.state ?? {};
  const kind = toolKindFromName(tool);
  return extractToolPreview(
    {
      title: typeof state.title === "string" ? state.title : tool,
      name: tool,
      kind,
      input: state.input,
      rawInput: state.input,
      content: state.output ?? state.metadata,
    },
    {
      title: tool,
      name: tool,
      kind,
      rawInput: state.input,
    },
  );
}

export function detailFromToolPart(part: OpenCodePart): string | undefined {
  const state = part.state ?? {};
  const status = typeof state.status === "string" ? state.status : "";
  if (status === "completed" && typeof state.output === "string") return state.output;
  if (status === "error" && typeof state.error === "string") return state.error;
  if (status === "running" && typeof state.title === "string") return state.title;
  return undefined;
}

export function permissionTitle(permission: string, patterns: string[]): string {
  const detail = patterns.length > 0 ? patterns.join("\n") : permission;
  switch (permission) {
    case "bash":
      return detail ? `Run ${detail}` : "Run command";
    case "edit":
      return detail ? `Edit ${detail}` : "Edit file";
    case "read":
      return detail ? `Read ${detail}` : "Read file";
    default:
      return detail || permission;
  }
}

export function sessionErrorMessage(error: unknown): string {
  if (!error || typeof error !== "object") return "OpenCode session failed.";
  const rec = error as Record<string, unknown>;
  const data = asRecord(rec.data);
  const message =
    stringField(data, "message") ??
    stringField(rec, "message") ??
    stringField(asRecord(rec.error), "message");
  return message ?? "OpenCode session failed.";
}

/**
 * Context level from an OpenCode assistant message `info`.
 *
 * Cached reads still occupy the window, so they count alongside fresh input;
 * output counts because it carries into the next request.
 */
export function contextUsedFromMessageInfo(
  info: Record<string, unknown> | null,
): number | undefined {
  const tokens = asRecord(info?.tokens);
  if (!tokens) return undefined;
  const cache = asRecord(tokens.cache);
  const num = (rec: Record<string, unknown> | null, key: string): number => {
    const value = rec?.[key];
    return typeof value === "number" && Number.isFinite(value) ? value : 0;
  };
  const used =
    num(tokens, "input") +
    num(tokens, "output") +
    num(tokens, "reasoning") +
    num(cache, "read") +
    num(cache, "write");
  return used > 0 ? used : undefined;
}

export function eventSessionId(event: Record<string, unknown>): string | undefined {
  const properties = asRecord(event.properties);
  if (!properties) return undefined;
  const sessionID = stringField(properties, "sessionID");
  if (sessionID) return sessionID;
  const info = asRecord(properties.info);
  return stringField(info, "id");
}

export function textDeltaEvent(
  part: OpenCodePart,
  text: string,
): HarnessEvent | null {
  if (!text) return null;
  if (part.type === "reasoning") return { type: "reasoning.delta", text };
  return { type: "message.delta", text };
}
