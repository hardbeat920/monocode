import type { AgentModel, ModelSetting } from "../../../../features/sessions/model/models";
import type { Attachment, RuntimeMode, ToolPreview } from "../../../../features/sessions/model/session";
import { attachmentPathText } from "../../../../features/sessions/model/attachments";
import { extractToolPreview, titleFromToolInput } from "../../core/preview";

/**
 * CommandCode (`cmd`) headless protocol.
 *
 * Each turn spawns `cmd -p <text> --output-format json …` and reads NDJSON
 * frames from stdout:
 *   {"type":"event","event":{"type":"tool_running",…}}  — progress frames
 *   {"type":"result","subtype":"success",…}              — always last
 *
 * Observed event types (unknown ones are ignored — the format is explicitly
 * forward-compatible): run_start, turn_start, message_start,
 * model_request_start, model_trace, text_delta, message_update,
 * model_request_end, message_end, tool_queued, tool_running, tool_completed,
 * tool_hook_blocked, turn_end, run_end.
 */

export type CmdSpawnInput = {
  text: string;
  model?: string;
  effort?: string;
  resume?: string;
  maxTurns?: number;
  runtimeMode: RuntimeMode;
  /** Read-only plan turn: `--plan` instead of the runtime-mode permissions. */
  plan?: boolean;
  /** Helper calls (titles, commit messages): no persistence, no onboarding. */
  noSession?: boolean;
  attachments?: Attachment[];
};

export function asRecord(value: unknown): Record<string, unknown> | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  return value as Record<string, unknown>;
}

export function strField(rec: Record<string, unknown>, key: string): string {
  const value = rec[key];
  return typeof value === "string" ? value : "";
}

export function numField(
  rec: Record<string, unknown>,
  key: string,
): number | null {
  const value = rec[key];
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

/** Join the text parts of a content array (`content`, `partial`, `result`). */
export function cmdContentText(value: unknown): string {
  if (typeof value === "string") return value;
  if (!Array.isArray(value)) return "";
  let out = "";
  for (const part of value) {
    const rec = asRecord(part);
    if (!rec || strField(rec, "type") !== "text") continue;
    out += strField(rec, "text");
  }
  return out;
}

/** Tool output can be a whole file; keep the transcript row scannable. */
export function truncateCmdDetail(text: string, limit = 4_000): string {
  const trimmed = text.trim();
  if (trimmed.length <= limit) return trimmed;
  return `${trimmed.slice(0, limit)}\n…[truncated ${trimmed.length - limit} chars]`;
}

/** Parse one stdout line; null when it is not a JSON object (progress text). */
export function parseCmdLine(line: string): Record<string, unknown> | null {
  const trimmed = line.trim();
  if (!trimmed.startsWith("{")) return null;
  try {
    return asRecord(JSON.parse(trimmed));
  } catch {
    return null;
  }
}

/** Top-level frame: "event" | "result" | null. */
export function cmdFrameType(rec: Record<string, unknown>): string | null {
  const type = strField(rec, "type");
  return type === "event" || type === "result" ? type : null;
}

export function cmdEvent(rec: Record<string, unknown>): Record<string, unknown> | null {
  if (cmdFrameType(rec) !== "event") return null;
  return asRecord(rec.event);
}

export function cmdEventType(rec: Record<string, unknown>): string {
  const event = cmdEvent(rec);
  return event ? strField(event, "type") : "";
}

export type CmdEventStatus = { key: string; text: string };

function attemptSuffix(attempt: number | null, max: number | null): string {
  if (attempt == null) return "";
  if (max == null) return ` (attempt ${attempt})`;
  return ` (${attempt}/${max})`;
}

/**
 * The CLI's own recovery notices: it retries model calls in-process and
 * restarts interrupted streams before ever failing a run. Surface them as
 * status rows (keyed, so each notice replaces the previous one) rather than
 * swallowing them as unknown events.
 */
export function cmdStatusFromEvent(
  event: Record<string, unknown>,
): CmdEventStatus | null {
  const attempt = numField(event, "attempt");
  const max = numField(event, "maxAttempts");
  switch (strField(event, "type")) {
    case "api_retry": {
      const counter = attempt == null ? "" : ` (attempt ${attempt})`;
      return {
        key: "cmd-connection",
        text: `Connection issue. Retrying…${counter}`,
      };
    }
    case "stream_restart":
      return {
        key: "cmd-connection",
        text: `Connection dropped mid-response. Retrying…${attemptSuffix(attempt, max)}`,
      };
    case "continuation_recovery":
      return {
        key: "cmd-connection",
        text: `Recovering an interrupted response${attemptSuffix(attempt, max)}.`,
      };
    default:
      return null;
  }
}

/**
 * Permission flags per runtime mode. Headless `cmd` has no UI to answer
 * prompts, so `--yolo` is the only flag that enables file writes and shell
 * commands in print mode. Supervised stays read-only; the auto and
 * full-access modes all map to `--yolo` because the CLI cannot ask mid-turn.
 */
export function cmdModeArgs(runtimeMode: RuntimeMode): string[] {
  switch (runtimeMode) {
    case "auto-accept-edits":
    case "auto":
    case "full-access":
      return ["--yolo"];
    case "supervised":
    default:
      return ["--permission-mode", "default"];
  }
}

export function buildCmdSpawnArgs(input: CmdSpawnInput): string[] {
  const args = ["-p", cmdPromptWithAttachments(input), "--output-format", "json"];
  if (input.noSession) {
    args.push("--no-session");
  } else if (input.resume) {
    args.push("--resume", input.resume);
  }
  args.push("--skip-onboarding", "--trust");
  if (input.plan) {
    args.push("--plan");
  } else {
    args.push(...cmdModeArgs(input.runtimeMode));
  }
  const model = input.model?.trim();
  if (model) args.push("--model", model);
  const effort = input.effort?.trim();
  if (effort) args.push("--effort", effort);
  if (input.maxTurns && input.maxTurns > 0) {
    args.push("--max-turns", String(Math.floor(input.maxTurns)));
  }
  return args;
}

/** `cmd -p` takes a single prompt string, so attachments ride along as path lines. */
export function cmdPromptWithAttachments(input: {
  text: string;
  attachments?: Attachment[];
}): string {
  const paths = (input.attachments ?? []).map((attachment) =>
    attachmentPathText(attachment),
  );
  return [input.text, ...paths].filter((part) => part.trim()).join("\n\n");
}

/**
 * The `cmd` CLI forwards referenced files to its OpenCode backend as file
 * parts, and that backend rejects some media types outright (observed:
 * `text/markdown` → "'file part media type text/markdown' functionality not
 * supported"). Text attachments are therefore inlined into the prompt as
 * fenced blocks so no file part is ever created for them. Images, folders,
 * audio/video, and opaque binaries keep traveling as path references.
 */
export const MAX_CMD_INLINE_CHARS = 50_000;

const CMD_INLINE_MIMES = new Set([
  "application/json",
  "application/xml",
  "application/sql",
  "application/graphql",
  "application/toml",
]);

export function isCmdInlineTextAttachment(attachment: Attachment): boolean {
  if (attachment.kind !== "file") return false;
  if (!attachment.path?.trim()) return false;
  const mime = attachment.mimeType.toLowerCase();
  if (mime === "inode/directory") return false;
  if (mime.startsWith("audio/") || mime.startsWith("video/")) return false;
  return mime.startsWith("text/") || CMD_INLINE_MIMES.has(mime);
}

/** Split attachments into text to inline vs. references that stay path lines. */
export function splitCmdAttachments(attachments: Attachment[]): {
  inline: Attachment[];
  referenced: Attachment[];
} {
  const inline: Attachment[] = [];
  const referenced: Attachment[] = [];
  for (const attachment of attachments ?? []) {
    (isCmdInlineTextAttachment(attachment) ? inline : referenced).push(
      attachment,
    );
  }
  return { inline, referenced };
}

function cmdInlineFence(language: string, content: string): string {
  let longest = 0;
  const runs = content.match(/`+/g);
  if (runs) {
    for (const run of runs) longest = Math.max(longest, run.length);
  }
  const fence = "`".repeat(Math.max(3, longest + 1));
  const tag = language ? fence + language : fence;
  return `${tag}\n${content}\n${fence}`;
}

function cmdInlineLanguage(name: string, mime: string): string {
  const dot = name.toLowerCase().lastIndexOf(".");
  const ext = dot > 0 ? name.toLowerCase().slice(dot + 1) : "";
  if (ext) return ext;
  if (mime === "application/json") return "json";
  if (mime === "application/xml") return "xml";
  if (mime === "application/sql") return "sql";
  return "text";
}

/** Render one inlined text attachment as a fenced block with truncation. */
export function formatCmdInlineAttachment(
  name: string,
  mime: string,
  content: string,
): string {
  const trimmed = content.replace(/\s+$/, "");
  const truncated = trimmed.length > MAX_CMD_INLINE_CHARS;
  const body = truncated
    ? `${trimmed.slice(0, MAX_CMD_INLINE_CHARS)}\n…[truncated ${trimmed.length - MAX_CMD_INLINE_CHARS} chars]`
    : trimmed;
  const quoted = JSON.stringify(name);
  return (
    `Attached file ${quoted} (contents inlined below — do not send this as a file part):\n` +
    cmdInlineFence(cmdInlineLanguage(name, mime.toLowerCase()), body)
  );
}

export function sessionIdFromCmdLine(rec: Record<string, unknown>): string | undefined {
  const event = cmdEvent(rec);
  if (event) {
    const id = strField(event, "sessionId");
    if (id) return id;
    const result = asRecord(event.result);
    if (result) {
      const nested = strField(result, "sessionId");
      if (nested) return nested;
      const next = asRecord(result.nextState);
      const nextId = next ? strField(next, "sessionId") : "";
      if (nextId) return nextId;
    }
  }
  if (cmdFrameType(rec) === "result") {
    const id = strField(rec, "sessionId");
    if (id) return id;
  }
  return undefined;
}

export type CmdToolRef = {
  callId: string;
  name: string;
  input: Record<string, unknown>;
};

export function toolRefFromCmdEvent(
  event: Record<string, unknown>,
): CmdToolRef | null {
  const callId =
    strField(event, "toolCallId") || strField(event, "id");
  const name = strField(event, "toolName") || strField(event, "name");
  if (!callId || !name) return null;
  return { callId, name, input: asRecord(event.input) ?? {} };
}

export function toolKindFromCmdName(toolName: string): string {
  const normalized = toolName.toLowerCase();
  if (
    normalized.includes("shell") ||
    normalized.includes("command") ||
    normalized.includes("powershell") ||
    normalized === "run_command"
  ) {
    return "shell";
  }
  if (normalized === "agent" || normalized.startsWith("agent_")) return "agent";
  if (normalized.startsWith("task_") || normalized === "todo_write") {
    return "task";
  }
  if (normalized === "ask_user_question") return "question";
  if (normalized.includes("edit") || normalized.includes("write")) {
    return "edit";
  }
  if (normalized.includes("read") || normalized.includes("directory")) {
    return "read";
  }
  if (
    normalized.includes("grep") ||
    normalized.includes("glob") ||
    normalized.includes("search") ||
    normalized.includes("fetch")
  ) {
    return "search";
  }
  return toolName;
}

function previewKindFromCmdName(toolName: string): ToolPreview["kind"] | undefined {
  const kind = toolKindFromCmdName(toolName);
  if (kind === "read") return "read";
  if (kind === "edit") return "write";
  if (kind === "shell") return "shell";
  if (kind === "search") return "search";
  return undefined;
}

export function cmdToolTitle(name: string, input: Record<string, unknown>): string {
  return titleFromToolInput(name, toolKindFromCmdName(name), input);
}

export function cmdPreviewFromTool(
  name: string,
  input: Record<string, unknown>,
): ToolPreview | undefined {
  const kind = previewKindFromCmdName(name);
  if (!kind) return undefined;
  return extractToolPreview(
    { title: name, name, kind, rawInput: input, input },
    { title: name, name, kind, rawInput: input },
  );
}

/** One-line detail for tool.updated rows: path, query, or command. */
export function summarizeCmdTool(
  name: string,
  input: Record<string, unknown>,
): string | undefined {
  const preview = cmdPreviewFromTool(name, input);
  if (preview?.path) return preview.path;
  if (preview?.query) return preview.query;
  for (const key of ["command", "cmd", "pattern", "query", "url", "file_path", "path"]) {
    const value = input[key];
    if (typeof value === "string" && value.trim()) {
      const first = value.trim().split(/\r?\n/)[0] ?? "";
      if (first) return first.slice(0, 120);
    }
  }
  return undefined;
}

export type CmdResultStatus = {
  status: "completed" | "failed";
  error?: string;
  maxTurns?: boolean;
};

/** Final {"type":"result"} line → turn outcome. */
export function resultStatusFromCmd(rec: Record<string, unknown>): CmdResultStatus {
  const subtype = strField(rec, "subtype");
  if (subtype === "success") return { status: "completed" };
  if (subtype === "max_turns") {
    return { status: "completed", maxTurns: true };
  }
  const error = strField(rec, "error") || "CommandCode turn failed";
  return { status: "failed", error };
}

/** The CLI's exit codes are its own classification; keep them on the error so
 *  retry decisions never depend on message string matching. */
const cmdExitCodes = new WeakMap<Error, number>();

export function cmdExitCodeOf(error: unknown): number | null {
  if (!(error instanceof Error)) return null;
  return cmdExitCodes.get(error) ?? null;
}

/**
 * Non-zero exits when no result line arrived. Codes from `cmd --help` /
 * headless docs: 3 auth, 4 permission, 5 rate limit, 6 network, 7 server,
 * 8 max turns, 10 credits, 130 interrupted.
 */
export function cmdExitError(code: number | null, stderrTail: string): Error {
  const error = cmdExitMessage(code, stderrTail);
  if (typeof code === "number") cmdExitCodes.set(error, code);
  return error;
}

function cmdExitMessage(code: number | null, stderrTail: string): Error {
  const tail = stderrTail.trim().split(/\r?\n/).slice(-3).join(" ").trim();
  const hint = tail ? ` ${tail.slice(0, 240)}` : "";
  switch (code) {
    case 3:
      return new Error(
        `CommandCode is not authenticated. Run \`cmd login\` in a terminal, then retry.${hint}`,
      );
    case 4:
      return new Error(`CommandCode denied permission.${hint}`);
    case 5:
      return new Error(`CommandCode rate limit exceeded.${hint}`);
    case 6:
      return new Error(`CommandCode network failure.${hint}`);
    case 7:
      return new Error(`CommandCode API server error.${hint}`);
    case 8:
      return new Error(`CommandCode hit its turn limit.${hint}`);
    case 10:
      return new Error(`CommandCode has insufficient credits.${hint}`);
    case 130:
      return new Error("CommandCode turn was interrupted.");
    default:
      return new Error(
        `CommandCode exited${code == null ? "" : ` with code ${code}`}.${hint}`,
      );
  }
}

/**
 * Connection-level failures worth respawning for. Covers the CLI's own exit
 * codes (6 connection, 7 API server) and the raw messages it surfaces in
 * result lines — notably `Error: Unable to connect to the API. Please check
 * your network connection.` and the undici/transport wording behind mid-stream
 * drops.
 */
export function isCmdTransientNetworkError(
  errorMessage: string,
  code?: number | null,
): boolean {
  if (code === 6 || code === 7) return true;
  const msg = errorMessage.toLowerCase();
  return (
    msg.includes("unable to connect to the api") ||
    msg.includes("check your network connection") ||
    msg.includes("network failure") ||
    msg.includes("api server error") ||
    msg.includes("network error") ||
    msg.includes("can't assign requested address") ||
    msg.includes("cant assign requested address") ||
    msg.includes("eaddrnotavail") ||
    msg.includes("connection reset") ||
    msg.includes("econnreset") ||
    msg.includes("broken pipe") ||
    msg.includes("epipe") ||
    msg.includes("network is unreachable") ||
    msg.includes("enetunreach") ||
    msg.includes("no route to host") ||
    msg.includes("ehostunreach") ||
    msg.includes("connection refused") ||
    msg.includes("econnrefused") ||
    msg.includes("socket hang up") ||
    msg.includes("socket disconnected") ||
    msg.includes("other side closed") ||
    msg.includes("econnaborted") ||
    msg.includes("etimedout") ||
    msg.includes("tls handshake timeout") ||
    msg.includes("i/o timeout") ||
    msg.includes("client.timeout exceeded") ||
    msg.includes("context deadline exceeded") ||
    msg.includes("fetch failed") ||
    msg.includes("unexpected eof") ||
    msg.includes("unexpected end of stream") ||
    msg.includes("stream ended")
  );
}

/**
 * Process-level retry envelope, mirroring the CLI's own `callModelWithRetry`
 * (10 attempts, backoff clamped to 1–10 s). Each attempt here is a full
 * `cmd -p` respawn because an in-process retry cannot survive a dead
 * transport.
 */
export const CMD_MAX_TURN_ATTEMPTS = 10;
const CMD_RETRY_BASE_MS = 100;
const CMD_RETRY_MIN_MS = 1_000;
const CMD_RETRY_MAX_MS = 10_000;

/** CLI backoff: min(10 s, max(1 s, 100 ms · 2^attempt)), attempt 0-based. */
export function cmdRetryDelayMs(attempt: number): number {
  const step = Math.min(Math.max(Math.floor(attempt), 0), 20);
  return Math.min(
    CMD_RETRY_MAX_MS,
    Math.max(CMD_RETRY_MIN_MS, CMD_RETRY_BASE_MS * 2 ** step),
  );
}

export function formatCmdErrorMessage(rawError: string): string {
  const trimmed = rawError.trim();
  if (isCmdTransientNetworkError(trimmed)) {
    const detail = trimmed.replace(/^error:\s*/i, "");
    return `CommandCode network error: ${detail}`;
  }
  const lower = trimmed.toLowerCase();
  if (lower.includes("response too big for into-string")) {
    return (
      "CommandCode couldn't read the provider response because it was too large. " +
      "Try again with smaller attachments or split the request into parts."
    );
  }
  const unsupported = lower.match(
    /file part media type\s+([^\s'"`]+).*functionality not supported/,
  );
  if (unsupported) {
    return (
      `CommandCode's backend doesn't accept ${unsupported[1]} file attachments. ` +
      "The file was sent as text instead — retry the turn and it should go through."
    );
  }
  return trimmed;
}

const CMD_NAME_WORDS: Record<string, string> = {
  ai: "AI",
  glm: "GLM",
  gpt: "GPT",
  hy: "HY",
  m: "M",
  v: "V",
};

const CMD_NAME_BRANDS: Record<string, string> = {
  deepseek: "DeepSeek",
  minimax: "MiniMax",
  minimaxai: "MiniMax",
  moonshotai: "Moonshot",
  qwen: "Qwen",
};

function prettyCmdModelName(nativeId: string): string {
  const base = nativeId.split("/").pop() ?? nativeId;
  const rawParts = base.split(/[-_]/).filter(Boolean);
  const parts: string[] = [];
  for (let index = 0; index < rawParts.length; index += 1) {
    const part = rawParts[index] ?? "";
    // "claude-sonnet-5-5" reads as 5.5, not "5 5".
    if (
      parts.length > 0 &&
      /^\d+$/.test(part) &&
      /^\d+$/.test(parts[parts.length - 1] ?? "")
    ) {
      parts[parts.length - 1] = `${parts[parts.length - 1]}.${part}`;
      continue;
    }
    const lower = part.toLowerCase();
    parts.push(
      CMD_NAME_BRANDS[lower] ??
        CMD_NAME_WORDS[lower] ??
        part.charAt(0).toUpperCase() + part.slice(1),
    );
  }
  return parts.join(" ");
}

/**
 * Reasoning-effort levels the `cmd` CLI accepts per model, probed with
 * `cmd -p --effort bogus-…` (`Unknown effort … Supported: …`). Effort is a
 * per-model setting because levels differ: Muse 1.3 contributor tops out at
 * xhigh, DeepSeek flash/pro take off/high/max, GLM 5.3 Flash takes
 * low/high/max (no medium/xhigh), MiniMax M3 has no xhigh at all, and
 * several models (GLM 5/5.1/5.2-fast, Kimi K2.5+, MiMo, Qwen 3.7, …) have no
 * adjustable effort. An empty list means no effort setting. Unknown ids fall
 * back to the full range minus max.
 */
const CMD_EFFORT_LABELS: Record<string, string> = {
  off: "Off",
  low: "Low",
  medium: "Medium",
  high: "High",
  xhigh: "Extra High",
  max: "Max",
};

/**
 * Reasoning-effort levels the `cmd` CLI accepts per model, probed against
 * CommandCode v1.87.1 with `cmd -p --effort bogus` (`Unknown effort …
 * Supported: …`). Levels differ per model and many have none, so an empty
 * list means the picker must not offer a Reasoning control. Unknown ids
 * also fall back to no setting rather than sending an unsupported level.
 */
export const CMD_EFFORT_OPTIONS: Record<string, string[]> = {
  "claude-fable-5": ["low", "medium", "high", "xhigh", "max"],
  "claude-fable-5-1": ["low", "medium", "high", "xhigh", "max"],
  "claude-haiku-4-5": [],
  "claude-haiku-5-5": ["low", "medium", "high", "xhigh", "max"],
  "claude-opus-4-7": ["low", "medium", "high", "xhigh", "max"],
  "claude-opus-4-8": ["low", "medium", "high", "xhigh", "max"],
  "claude-opus-5": ["low", "medium", "high", "xhigh", "max"],
  "claude-opus-5-5": ["low", "medium", "high", "xhigh", "max"],
  "claude-sonnet-4-6": ["low", "medium", "high", "xhigh", "max"],
  "claude-sonnet-5": ["low", "medium", "high", "xhigh", "max"],
  "claude-sonnet-5-5": ["low", "medium", "high", "xhigh", "max"],
  "deepseek/deepseek-v4-flash": ["off", "high", "max"],
  "deepseek/deepseek-v4-flash-fast": ["low", "high", "max"],
  "deepseek/deepseek-v4-flash-vision-exp": ["off", "high", "max"],
  "deepseek/deepseek-v4-pro": ["off", "high", "max"],
  "deepseek/deepseek-v4.1-flash": ["off", "low", "high", "max"],
  "deepseek/deepseek-v4.1-flash-fast": ["off", "low", "high", "max"],
  "google/gemini-3.1-flash-lite": ["low", "medium", "high"],
  "google/gemini-3.5-flash": ["low", "medium", "high"],
  "google/gemini-3.5-flash-lite": ["low", "medium", "high"],
  "google/gemini-3.6-flash": ["low", "medium", "high"],
  "google/gemini-3.7-flash": ["low", "medium", "high"],
  "google/gemini-3.8-flash": ["low", "medium", "high"],
  "google/gemini-4-argon": ["low", "medium", "high"],
  "gpt-5.3-codex": ["low", "medium", "high", "xhigh"],
  "gpt-5.4": ["low", "medium", "high", "xhigh"],
  "gpt-5.4-mini": ["low", "medium", "high"],
  "gpt-5.5": ["low", "medium", "high", "xhigh"],
  "gpt-5.6-luna": ["low", "medium", "high", "xhigh", "max"],
  "gpt-5.6-sol": ["low", "medium", "high", "xhigh", "max"],
  "gpt-5.6-terra": ["low", "medium", "high", "xhigh", "max"],
  "gpt-6-astra": ["low", "medium", "high", "xhigh", "max"],
  "gpt-6-luna": ["low", "medium", "high", "xhigh", "max"],
  "gpt-6-sol": ["low", "medium", "high", "xhigh", "max"],
  "gpt-6.1-sol": ["low", "medium", "high", "xhigh", "max"],
  "inclusionai/ling-3.1-flash": ["low", "medium", "high"],
  "meituan/longcat-2.0": [],
  "meta/muse-spark-1.1": ["low", "medium", "high", "xhigh"],
  "meta/muse-spark-1.2": ["low", "medium", "high", "xhigh"],
  "meta/muse-spark-1.2-contributor": ["low", "medium", "high", "xhigh"],
  "meta/muse-spark-1.3": ["low", "medium", "high", "xhigh", "max"],
  "meta/muse-spark-1.3-contributor": ["low", "medium", "high", "xhigh"],
  "meta/muse-spark-open": ["low", "medium", "high"],
  "meta/muse-wonder-1.0": ["low", "medium", "high", "xhigh"],
  "minimax/minimax-m3.1-flash": ["low", "medium", "high", "xhigh", "max"],
  "minimax/minimax-m3.1-flash:free": ["low", "medium", "high", "xhigh", "max"],
  "minimaxai/minimax-m2.5": [],
  "minimaxai/minimax-m2.7": [],
  "minimaxai/minimax-m3": ["low", "medium", "high"],
  "mistral/mistral-large-4": ["low", "medium", "high"],
  "moonshotai/kimi-k2.5": [],
  "moonshotai/kimi-k2.6": [],
  "moonshotai/kimi-k2.7-code": [],
  "moonshotai/kimi-k2.7-code-highspeed": [],
  "moonshotai/kimi-k3": ["low", "high", "max"],
  "nvidia/nemotron-3-ultra-550b-a55b": [],
  "nvidia/nemotron-3.5-super-vl": ["low", "medium", "high"],
  "ornith/ornith-1.5-122b": [],
  "ornith/ornith-1.5-122b:free": [],
  "poolside/laguna-s-2.1-free": [],
  "qwen/qwen3.6-max-preview": [],
  "qwen/qwen3.6-plus": [],
  "qwen/qwen3.7-flash": [],
  "qwen/qwen3.7-max": [],
  "qwen/qwen3.7-plus": [],
  "qwen/qwen3.8-27b": ["low", "medium", "xhigh"],
  "qwen/qwen3.8-flash": ["low", "medium", "xhigh"],
  "qwen/qwen3.8-max": ["low", "medium", "xhigh"],
  "qwen/qwen3.8-max-0902": ["low", "medium", "xhigh"],
  "qwen/qwen3.8-max-prime": ["low", "medium", "xhigh"],
  "qwen/qwen3.8-omni-flash": ["low", "medium", "xhigh"],
  "reflection/beam:free": ["low", "medium", "high", "xhigh", "max"],
  "sakana/fugu-ultra": ["high", "xhigh"],
  "stealth/glyph-cluster:free": ["low", "medium", "high", "xhigh"],
  "stepfun/step-3.5-flash": [],
  "stepfun/step-3.7-flash": [],
  "stepfun/step-5-preview": ["low", "medium", "high"],
  "tencent/hy3-paid": [],
  "tencent/hy4-preview": ["low", "medium", "high"],
  "thinkingmachines/inkling": [],
  "thinkingmachines/inkling-small": [],
  "xai/grok-4.5": ["low", "medium", "high"],
  "xai/grok-4.6": ["low", "medium", "high", "xhigh"],
  "xai/grok-4.7": ["low", "medium", "high", "xhigh"],
  "xiaomi/mimo-v2.5": [],
  "xiaomi/mimo-v2.5-pro": [],
  "xiaomi/mimo-v2.6-flash": [],
  "xiaomi/mimo-v2.6-pro": [],
  "xiaomi/mimo-v2.6-pro-ultraspeed": [],
  "z-ai/glm-5.3-flash": ["low", "high", "max"],
  "z-ai/glm-5.3-flashx": ["low", "high", "max"],
  "zai-org/glm-5": [],
  "zai-org/glm-5.1": [],
  "zai-org/glm-5.2": ["high", "max"],
  "zai-org/glm-5.2-fast": [],
  "zai-org/glm-5.3": ["low", "high", "max"],
  // typesafe/jev: no adjustable effort reported
};

const CMD_DEFAULT_EFFORT: Record<string, string> = {
  "meta/muse-spark-1.3-contributor": "xhigh",
};

function cmdEffortSetting(nativeId: string): ModelSetting | null {
  const short = nativeId.includes("/") ? nativeId : `meta/${nativeId}`;
  const bare = short.split("/").pop() ?? short;
  const levels =
    CMD_EFFORT_OPTIONS[short] ?? CMD_EFFORT_OPTIONS[bare] ?? [];
  if (levels.length === 0) return null;
  const options = levels.map((value) => ({
    value,
    label: CMD_EFFORT_LABELS[value] ?? value,
  }));
  const fallback = levels.includes("high") ? "high" : (levels[0] ?? "high");
  const value =
    CMD_DEFAULT_EFFORT[short] ?? CMD_DEFAULT_EFFORT[bare] ?? fallback;
  return {
    id: "effort",
    label: "Reasoning",
    kind: "select",
    value: options.some((option) => option.value === value) ? value : fallback,
    options,
  };
}

const CMD_MODEL_ID = /^[A-Za-z0-9_.:+@-]+(?:\/[A-Za-z0-9_.:+@-]+)?$/;

/**
 * One model row in `cmd --list-models`: a native id followed by a
 * space-padded description. Some providers use namespaced ids
 * (`deepseek/deepseek-v4-flash`) and others use bare ids
 * (`claude-sonnet-5-5`), so a bare id needs a description column to be
 * distinguishable from a section header.
 */
function cmdModelIdFromListLine(line: string): string | null {
  const first = line.split(/\s+/)[0] ?? "";
  if (!CMD_MODEL_ID.test(first) || first.endsWith(":")) return null;
  const namespaced = first.includes("/");
  const hasDescription = /^\S+ {2,}\S/.test(line);
  if (!namespaced && !hasDescription) return null;
  // Headers and prose ("Pass the full id…") are never ids; every real bare
  // model id carries a version number (claude-sonnet-5-5, gpt-6-astra).
  if (!namespaced && !/\d/.test(first)) return null;
  return first;
}

/**
 * Parse `cmd --list-models` text output. Lines look like:
 *   deepseek/deepseek-v4-flash   fast hybrid-attention reasoning (default)
 *   claude-sonnet-5-5            best combo of speed & intelligence
 * Section headers and the trailing decision-model section are skipped.
 */
export function modelsFromCmdListOutput(stdout: string): AgentModel[] {
  const models: AgentModel[] = [];
  const seen = new Set<string>();
  for (const rawLine of stdout.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (!line) continue;
    if (/^available models/i.test(line)) continue;
    // Decision models take structured JSON, not chat turns.
    if (/^decision models/i.test(line)) break;
    const first = cmdModelIdFromListLine(line);
    if (!first || seen.has(first)) continue;
    seen.add(first);
    const effort = cmdEffortSetting(first);
    models.push({
      id: `cmd:${first}`,
      harness: "cmd",
      name: prettyCmdModelName(first),
      nativeId: first,
      ...(effort ? { settings: [effort] } : {}),
    });
  }
  return models;
}

/** Native id flagged `(default)` in `cmd --list-models`, if any. */
export function defaultModelFromCmdListOutput(stdout: string): string | null {
  for (const rawLine of stdout.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (!/\(default\)/i.test(line)) continue;
    const first = cmdModelIdFromListLine(line);
    if (first) return first;
  }
  return null;
}
