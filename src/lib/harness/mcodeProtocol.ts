import type { PromptContentBlock } from "../attachments";
import type { AgentModel, ModelSetting } from "../models";
import type { RuntimeMode, TaskListItem, ToolPreview } from "../session";
import { normalizeTaskListStatus } from "../taskList";
import type { ApprovalDecision, HarnessEvent } from "./types";

export type McodeModeId = "ask" | "code" | "plan" | "build";

export type McodePermissionRequest = {
  title: string;
  kind?: string;
  callId?: string;
  preview?: ToolPreview;
  optionIds: string[];
};

export type SessionConfigOption = {
  id: string;
  category?: string;
  currentValue?: string | boolean;
};

type Record_ = Record<string, unknown>;

function asRecord(value: unknown): Record_ | null {
  return value && typeof value === "object"
    ? (value as Record_)
    : null;
}

function stringField(
  ...args: Array<Record_ | null | undefined | string>
): string | undefined {
  // Split positional args into records (objects to read from) and key
  // names (strings naming the field to read). Reading every key of the
  // first record in insertion order — instead of only the requested
  // keys — was returning the wrong field for `toolCallId`/`title`/`kind`
  // /`sessionId`/etc. and was incorrectly auto-allowing write-class tools
  // in plan mode.
  const records = args.filter(
    (arg): arg is Record_ => Boolean(arg) && typeof arg === "object",
  );
  const keys = args.filter((arg): arg is string => typeof arg === "string");
  for (const record of records) {
    for (const key of keys) {
      const value = record[key];
      if (typeof value === "string" && value.length > 0) return value;
    }
  }
  return undefined;
}

function pickOption(optionIds: string[], preferred: string[]): string | null {
  for (const candidate of preferred) {
    const hit = optionIds.find(
      (id) => id.toLowerCase() === candidate.toLowerCase(),
    );
    if (hit) return hit;
  }
  return null;
}

function textFromContent(content: unknown, fallback = ""): string {
  if (typeof content === "string") return content;
  if (Array.isArray(content)) {
    return content
      .map((block) => {
        const rec = asRecord(block);
        if (!rec) return "";
        if (typeof rec.text === "string") return rec.text;
        if (typeof rec.content === "string") return rec.content;
        return "";
      })
      .filter(Boolean)
      .join("");
  }
  if (content && typeof content === "object") {
    const rec = asRecord(content);
    if (rec && typeof rec.text === "string") return rec.text;
  }
  return fallback;
}

/** mcode accepts text + image prompt blocks (mirrors the standard ACP shape). */
export function mcodePromptBlocks(text: string): PromptContentBlock[] {
  const trimmed = text.trim();
  return trimmed ? [{ type: "text", text: trimmed }] : [];
}

/** Map the runtime mode to mcode's ACP mode id. */
/** Map the runtime mode to mcode's ACP mode id. */
export function mcodeModeId(runtimeMode: RuntimeMode): McodeModeId {
  switch (runtimeMode) {
    case "supervised":
      return "ask";
    case "auto-accept-edits":
    case "auto":
    case "full-access":
      return "code";
    default:
      return "code";
  }
}

/**
 * Auto-approve any permission request that still reaches us.
 * Picks the most permissive option id from the option list.
 */
export function mcodeAutoPermissionOption(
  _runtimeMode: RuntimeMode,
  optionIds: string[],
): string | null {
  if (optionIds.length === 0) return null;
  return pickOption(optionIds, [
    "allow-always",
    "allow_always",
    "allow-once",
    "allow_once",
    "allow",
  ]);
}

/**
 * Map an ApprovalDecision to the matching option id mcode offered.
 * Falls back to `allow-once` / `reject-once` if the option list uses
 * a non-canonical label we don't recognize.
 */
export function mcodePermissionOptionId(
  decision: ApprovalDecision,
  optionIds: string[],
): string {
  if (decision === "allow") {
    return (
      pickOption(optionIds, [
        "allow-once",
        "allow_once",
        "allow-always",
        "allow_always",
        "allow",
      ]) ?? "allow-once"
    );
  }
  return (
    pickOption(optionIds, [
      "reject-once",
      "reject_once",
      "reject-always",
      "reject_always",
      "reject",
    ]) ?? "reject-once"
  );
}

/**
 * Parse an ACP `session/request_permission` notification into the
 * shape the rest of the harness expects: title, optional tool kind,
 * call id, and the list of option ids the agent offered.
 */
export function mcodePermissionRequestFromAcp(
  params: unknown,
): McodePermissionRequest {
  const rec = asRecord(params) ?? {};
  const toolCall = asRecord(rec.toolCall) ?? asRecord(rec.tool_call);
  const callId =
    stringField(toolCall, rec, "toolCallId", "tool_call_id") ?? undefined;
  const kind = stringField(toolCall, rec, "kind") ?? undefined;
  const title =
    stringField(toolCall, rec, "title", "name") ?? "Tool call";
  const options = Array.isArray(rec.options) ? rec.options : [];
  const optionIds = options
    .map((option) => {
      const r = asRecord(option);
      return typeof r?.optionId === "string"
        ? (r.optionId as string)
        : typeof r?.id === "string"
          ? (r.id as string)
          : null;
    })
    .filter((value): value is string => Boolean(value));
  return { title, kind, callId, optionIds };
}

/**
 * Flatten the raw `configOptions` array from a session/new or session/set_config_option
 * response into the {id, category, currentValue} records the harness expects.
 */
export function mcodeReadConfigOptions(raw: unknown): SessionConfigOption[] {
  if (!Array.isArray(raw)) return [];
  return raw.flatMap((item) => {
    const rec = asRecord(item);
    const id = String(rec?.id ?? rec?.configId ?? "").trim();
    if (!id) return [];
    return [
      {
        id,
        category: typeof rec?.category === "string" ? rec.category : undefined,
        currentValue:
          typeof rec?.currentValue === "string" ||
          typeof rec?.currentValue === "boolean"
            ? rec.currentValue
            : undefined,
      },
    ];
  });
}

/**
 * Pick the config option id that selects the model. Prefers the literal
 * "model" id; falls back to the first category=model option that isn't the
 * provider picker, then to "model".
 */
export function mcodeExtractModelConfigId(
  options: SessionConfigOption[],
): string {
  const exact = options.find((option) => option.id === "model");
  if (exact) return exact.id;
  const model = options.find(
    (option) => option.category === "model" && option.id !== "provider",
  );
  return model?.id ?? "model";
}

/**
 * Resolve a user-visible setting id (e.g. "effort") to the corresponding
 * mcode config option id. Falls back to aliases like "reasoning" or
 * category "thought_level" when the exact id isn't present.
 */
export function mcodeResolveSettingConfigId(
  options: SessionConfigOption[],
  settingId: string,
): string | undefined {
  const needle = settingId.trim().toLowerCase();
  const exact = options.find((option) => option.id.toLowerCase() === needle);
  if (exact) return exact.id;
  if (needle === "effort" || needle === "reasoning") {
    return options.find(
      (option) =>
        option.id === "effort" ||
        option.id === "reasoning" ||
        option.category === "thought_level",
    )?.id;
  }
  return undefined;
}

/**
 * Extract the ACP session id from a session/new or session/resume response.
 * Returns undefined if the response doesn't carry an id; the caller is
 * responsible for treating that as a hard failure (mcode is required to
 * return a session id on session/new).
 */
export function mcodeSessionIdFromResult(result: unknown): string | undefined {
  const rec = asRecord(result);
  if (!rec) return undefined;
  const direct =
    stringField(rec, "sessionId", "session_id", "id");
  return direct;
}

/**
 * Convert an ACP `session/update` notification into the app's
 * `HarnessEvent` stream: agent message / thought chunks become
 * `message.delta` / `reasoning.delta`; tool calls and plan entries
 * become `tool.updated` / `plan` rows.
 */
export function mcodeEventsFromAcpUpdate(params: unknown): HarnessEvent[] {
  const rec = asRecord(params);
  const update = asRecord(rec?.update) ?? rec;
  if (!update) return [];
  const kind = String(
    update.sessionUpdate ?? update.session_update ?? update.type ?? "",
  );

  if (kind === "agent_message_chunk" || kind === "agent_message") {
    const text = textFromContent(
      update.content ?? update.text,
      kind === "agent_message" ? "\n" : "",
    );
    return text ? [{ type: "message.delta", text }] : [];
  }

  if (kind === "agent_thought_chunk" || kind === "agent_thought") {
    const text = textFromContent(
      update.content ?? update.text,
      kind === "agent_thought" ? "\n" : "",
    );
    return text ? [{ type: "reasoning.delta", text }] : [];
  }

  if (kind === "tool_call" || kind === "tool_call_update") {
    const tool = asRecord(update.toolCall) ?? asRecord(update.tool_call) ?? update;
    const callId = stringField(
      tool,
      update,
      "toolCallId",
      "tool_call_id",
    );
    if (!callId) return [];
    const title = stringField(tool, update, "title", "name") ?? "Tool call";
    const toolKind = stringField(tool, update, "kind");
    const status = stringField(update, "status") ?? stringField(tool, "status");
    return [
      {
        type: "tool.updated",
        callId,
        title,
        kind: toolKind,
        status,
      },
    ];
  }

  if (kind === "plan") {
    // ACP `plan` notifications carry an `entries: [{content, priority, status}]`
    // array — read the entries directly instead of via the loose `stringField`
    // helper. Each entry becomes a task in a `tasks.updated` event, matching
    // the canonical harness contract (fxProtocol.ts:planEvent) so the UI
    // can render the plan as a todo list rather than freeform text.
    const entriesRaw = update.entries;
    if (Array.isArray(entriesRaw)) {
      const items = entriesRaw.flatMap((entry): TaskListItem[] => {
        const rec = asRecord(entry);
        if (!rec) return [];
        const content =
          (typeof rec.content === "string" ? rec.content : undefined) ??
          stringField(rec, "text");
        if (!content) return [];
        return [
          {
            text: content,
            status: normalizeTaskListStatus(rec.status),
          } as TaskListItem,
        ];
      });
      if (items.length > 0) {
        return [{ type: "tasks.updated", items }];
      }
    }
    // Some legacy/proprietary providers may still emit plan as a single
    // text blob — fall back to that shape rather than dropping the update.
    const fallback = stringField(update, "plan") ?? stringField(update, "text");
    return fallback ? [{ type: "plan", text: fallback, append: true }] : [];
  }

  return [];
}

/**
 * Map mcode's structured config options into the app's ModelSetting
 * shape so the picker can render toggles and select controls. Skips
 * the model and provider options — the app already has dedicated UI
 * for those.
 */
export function mcodeConfigToModelSettings(
  options: SessionConfigOption[],
): ModelSetting[] {
  const out: ModelSetting[] = [];
  for (const option of options) {
    if (!option.category) continue;
    if (option.id === "model" || option.id === "provider") continue;
    if (typeof option.currentValue === "boolean") {
      out.push({
        id: option.id,
        label: option.id,
        kind: "toggle",
        value: option.currentValue ? "true" : "false",
        options: [
          { value: "true", label: "On" },
          { value: "false", label: "Off" },
        ],
      });
    } else if (typeof option.currentValue === "string") {
      out.push({
        id: option.id,
        label: option.id,
        kind: "select",
        value: option.currentValue,
        options: [{ value: option.currentValue, label: option.currentValue }],
      });
    }
  }
  return out;
}

/**
 * Filter a MODELS list down to those the mcode runtime actually exposes.
 * Currently a no-op because mcode reports its active model through
 * session config; this hook exists so the picker can drop unsupported
 * entries once a runtime query is added.
 */
export function mcodeFilterModels(models: AgentModel[]): AgentModel[] {
  // mcode reports its current model through session config; we still let the
  // user pick from the catalog and forward the choice to the runtime.
  return models;
}
