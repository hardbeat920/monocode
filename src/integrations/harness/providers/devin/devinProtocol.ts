import { promptBlocks, type PromptContentBlock } from "../../../../features/sessions/model/attachments";
import type { AgentModel } from "../../../../features/sessions/model/models";
import type { Attachment, RuntimeMode, ToolPreview } from "../../../../features/sessions/model/session";
import { nativeCommandInvocation, type NativeCommand } from "../../core/nativeCommands";
import type { HarnessEvent } from "../../core/types";
import { asRecord, eventsFromAcpUpdate, stringField } from "../antigravity/antigravityProtocol";

export const DEVIN_AUTH_HELP =
  "Run `devin auth login` in a terminal, then retry.";

/** Devin only advertises a browser method; an API key rides on its `_meta`. */
export const DEVIN_AUTH_METHOD = "devin-browser";

export type DevinModeId = "accept-edits" | "smart" | "plan" | "bypass";

/**
 * Devin's ACP server has no "ask before every edit" mode: its most careful
 * coding mode still accepts edits and prompts for commands. Plan intent uses
 * Devin's native plan mode; client-side gating still denies writes.
 */
export function devinModeId(
  runtimeMode: RuntimeMode,
  planning = false,
): DevinModeId {
  if (planning) return "plan";
  if (runtimeMode === "full-access") return "bypass";
  if (runtimeMode === "auto") return "smart";
  return "accept-edits";
}

/** `devin auth status` prints where `devin auth login` stored credentials. */
export function devinCredentialsPathFromStatus(stdout: string): string | null {
  const match = stdout.match(/^\s*Credentials path:\s*(.+?)\s*$/im);
  return match?.[1] ? match[1] : null;
}

/** Default credential locations when `devin auth status` is unavailable. */
export function devinCredentialsCandidates(home: string): string[] {
  const base = home.replace(/[\\/]+$/, "");
  return [
    `${base}/.config/devin/credentials.toml`,
    `${base}/AppData/Roaming/devin/credentials.toml`,
  ];
}

/** Read the API key `devin auth login` writes to credentials.toml. */
export function devinApiKeyFromCredentials(toml: string): string | null {
  for (const key of ["windsurf_api_key", "devin_api_key", "api_key"]) {
    const match = toml.match(
      new RegExp(`^\\s*${key}\\s*=\\s*(?:"([^"\\r\\n]+)"|'([^'\\r\\n]+)')`, "m"),
    );
    const value = (match?.[1] ?? match?.[2])?.trim();
    if (value) return value;
  }
  return null;
}

/** Prefer the method the server advertised, falling back to Devin's id. */
export function devinAuthMethodId(init: unknown): string {
  const methods = asRecord(init)?.authMethods;
  if (Array.isArray(methods)) {
    for (const method of methods) {
      const id = stringField(asRecord(method) ?? {}, "id");
      if (id) return id;
    }
  }
  return DEVIN_AUTH_METHOD;
}

export function devinAuthenticateParams(
  methodId: string,
  apiKey: string | null,
): Record<string, unknown> {
  return apiKey
    ? { methodId, _meta: { api_key: apiKey } }
    : { methodId };
}

export function devinPromptBlocks(
  text: string,
  attachments: Attachment[] = [],
): PromptContentBlock[] {
  return promptBlocks(text, attachments);
}

export function devinStartupError(error: unknown): Error {
  const detail = (error instanceof Error ? error.message : String(error)).trim();
  if (detail.includes(DEVIN_AUTH_HELP)) return new Error(detail);
  if (/auth|credential|api key|log ?in|sign.in/i.test(detail)) {
    return new Error(`${detail}\n\n${DEVIN_AUTH_HELP}`);
  }
  if (/timed out/i.test(detail)) {
    return new Error(`Devin did not start. ${DEVIN_AUTH_HELP}`);
  }
  return new Error(`Devin did not start. ${detail}`);
}

/** Models come from the `model` select in Devin's session config options. */
export function modelsFromDevinSession(raw: unknown): AgentModel[] {
  const rec = asRecord(raw);
  const options = Array.isArray(rec?.configOptions) ? rec.configOptions : [];
  const option = options
    .map(asRecord)
    .find((item) => item?.id === "model" || item?.category === "model");
  const choices = flattenChoices(option?.options);
  const current =
    typeof option?.currentValue === "string" ? option.currentValue : "";
  const seen = new Set<string>();
  const models: AgentModel[] = choices.flatMap(({ value, label }) => {
    if (seen.has(value)) return [];
    seen.add(value);
    return [{ id: `devin:${value}`, harness: "devin" as const, name: label, nativeId: value }];
  });
  // setHarnessModels defaults to the first entry, so lead with the model the
  // account is already configured to use.
  const index = models.findIndex((model) => model.nativeId === current);
  if (index > 0) models.unshift(...models.splice(index, 1));
  return models;
}

function flattenChoices(raw: unknown): Array<{ value: string; label: string }> {
  if (!Array.isArray(raw)) return [];
  return raw.flatMap((item) => {
    const rec = asRecord(item);
    if (!rec) return [];
    if (Array.isArray(rec.options)) return flattenChoices(rec.options);
    const value = stringField(rec, "value");
    if (!value) return [];
    return [{ value, label: String(rec.name ?? rec.label ?? value) }];
  });
}

function updateOf(params: unknown): Record<string, unknown> | null {
  const rec = asRecord(params);
  return asRecord(rec?.update) ?? rec;
}

function updateKind(update: Record<string, unknown> | null): string {
  return String(update?.sessionUpdate ?? update?.session_update ?? "");
}

const ANSI = /\u001b\[[0-?]*[ -/]*[@-~]|\u001b\][^\u0007]*(?:\u0007|\u001b\\)/g;

export function stripAnsi(text: string): string {
  return text.replace(ANSI, "");
}

function cleanPreview(preview: ToolPreview | undefined): ToolPreview | undefined {
  if (!preview?.output) return preview;
  return { ...preview, output: stripAnsi(preview.output) };
}

/**
 * Standard ACP updates, with Devin's terminal colour codes removed and its
 * token accounting (carried in `_meta`) surfaced as turn metrics.
 */
export function devinEventsFromUpdate(params: unknown): HarnessEvent[] {
  const update = updateOf(params);
  const events = eventsFromAcpUpdate(params).map((event): HarnessEvent => {
    if (event.type !== "tool.updated") return event;
    return {
      ...event,
      ...(event.detail ? { detail: stripAnsi(event.detail) } : {}),
      ...(event.preview ? { preview: cleanPreview(event.preview) } : {}),
    };
  });
  if (updateKind(update) !== "usage_update") return events;
  const meta = asRecord(update?._meta);
  const number = (key: string) => {
    const value = meta?.[`cognition.ai/${key}`];
    return typeof value === "number" && Number.isFinite(value) ? value : undefined;
  };
  const inputTokens = number("inputTokens");
  const outputTokens = number("outputTokens");
  const cacheReadTokens = number("cachedReadTokens");
  if (inputTokens == null && outputTokens == null) return events;
  const cacheable = (inputTokens ?? 0) + (cacheReadTokens ?? 0);
  return [
    ...events.filter((event) => event.type !== "turn.metrics"),
    {
      type: "turn.metrics",
      ...(inputTokens != null ? { inputTokens } : {}),
      ...(outputTokens != null ? { outputTokens } : {}),
      ...(cacheReadTokens != null ? { cacheReadTokens } : {}),
      ...(cacheReadTokens != null && cacheable > 0
        ? { cacheHitPercent: (cacheReadTokens / cacheable) * 100 }
        : {}),
    },
  ];
}

/** The tool row a permission request refers to, remembered from tool_call. */
export type DevinToolInfo = {
  title?: string;
  kind?: string;
  preview?: ToolPreview;
};

export function devinToolInfo(
  params: unknown,
): { callId: string; info: DevinToolInfo } | null {
  for (const event of devinEventsFromUpdate(params)) {
    if (event.type !== "tool.updated" || !event.callId) continue;
    return {
      callId: event.callId,
      info: { title: event.title, kind: event.kind, preview: event.preview },
    };
  }
  return null;
}

/** Devin's permission requests carry the editable command in `_meta`. */
export function devinPermissionCommand(params: unknown): string | undefined {
  const tool = asRecord(asRecord(params)?.toolCall);
  const meta = asRecord(tool?._meta);
  const command = meta?.["cognition.ai/editableCommand"];
  return typeof command === "string" && command.trim() ? command.trim() : undefined;
}

/**
 * Devin names the session itself. The first update is the truncated prompt
 * and transient ones can leak a raw tool call, so only accept a settled title.
 */
export function devinSessionTitle(params: unknown): string | null {
  const update = updateOf(params);
  if (updateKind(update) !== "session_info_update") return null;
  const title = typeof update?.title === "string" ? update.title.trim() : "";
  if (!title || /(?:\.\.\.|…)$/.test(title)) return null;
  if (/^functions\.|[{}]/.test(title)) return null;
  return title;
}

/**
 * Slash commands from `available_commands_update`. Devin also lists every
 * installed skill there; MonoCode already discovers those from disk.
 */
export function devinCommandsFromUpdate(params: unknown): NativeCommand[] | null {
  const update = updateOf(params);
  if (updateKind(update) !== "available_commands_update") return null;
  const raw = update?.availableCommands ?? update?.available_commands;
  if (!Array.isArray(raw)) return null;
  const seen = new Set<string>();
  return raw.flatMap((item) => {
    const rec = asRecord(item);
    const name = stringField(rec ?? {}, "name")?.trim();
    if (!rec || !name || seen.has(name)) return [];
    const category = asRecord(rec._meta)?.["cognition.ai/category"];
    if (category === "Skills") return [];
    seen.add(name);
    const hint = stringField(asRecord(rec.input) ?? {}, "hint");
    return [
      {
        name,
        description: String(rec.description ?? "").trim(),
        invocation: nativeCommandInvocation("devin", name),
        source: "devin" as const,
        ...(typeof category === "string" ? { origin: category } : {}),
        ...(hint ? { inputHint: hint } : {}),
      },
    ];
  });
}

export function devinAgentMessageText(params: unknown): string {
  return devinEventsFromUpdate(params)
    .map((event) => (event.type === "message.delta" ? event.text : ""))
    .join("");
}
