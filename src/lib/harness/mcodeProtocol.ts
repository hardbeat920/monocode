import type { PromptContentBlock } from "../attachments";
import type { AgentModel, ModelSetting } from "../models";
import type { RuntimeMode, ToolPreview } from "../session";
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
  ...records: Array<Record_ | null | undefined | string>
): string | undefined {
  for (const record of records) {
    if (!record || typeof record !== "object") continue;
    for (const key of Object.keys(record)) {
      const value = (record as Record_)[key];
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

/** Auto-approve any permission request that still reaches us. */
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

export function mcodeSessionIdFromResult(result: unknown): string | undefined {
  const rec = asRecord(result);
  if (!rec) return undefined;
  const direct =
    stringField(rec, "sessionId", "session_id", "id");
  return direct;
}

/** Convert an ACP `session/update` notification into HarnessEvents. */
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
    const plan = stringField(update, "plan") ?? stringField(update, "text");
    return plan ? [{ type: "plan", text: plan, append: true }] : [];
  }

  return [];
}

/** Map mcode's config options to the app's ModelSetting shape. */
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

/** Filter a MODELS list down to those the mcode runtime actually exposes. */
export function mcodeFilterModels(models: AgentModel[]): AgentModel[] {
  // mcode reports its current model through session config; we still let the
  // user pick from the catalog and forward the choice to the runtime.
  return models;
}
