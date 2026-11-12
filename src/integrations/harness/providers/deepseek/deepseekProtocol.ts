import type { PromptContentBlock } from "../../../../features/sessions/model/attachments";
import type {
  AgentModel,
  ModelSetting,
  ModelSettingChoice,
} from "../../../../features/sessions/model/models";
import type { RuntimeMode, ToolPreview } from "../../../../features/sessions/model/session";
import { normalizeTaskListStatus } from "../../../../features/sessions/model/taskList";
import type { ApprovalDecision, HarnessEvent } from "../../core/types";
import {
  composeToolTitle,
  extractSearchQuery,
  extractShellCommand,
  extractSkillName,
  extractToolPreview,
} from "../../core/preview";
import { acpAgentInfo } from "../../core/acpSubagents";
import { translate } from "../../../../shared/i18n/locale";

export const AUTH_HELP =
  "DeepSeek is not signed in. Open Settings → Accounts and complete the official DeepSeek OAuth, then retry.";

export type DeepseekPermissionRequest = {
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

const EFFORT_LABELS: Record<string, string> = {
  off: "Off",
  low: "Low",
  high: "High",
  max: "Max",
};

const DEFAULT_PROVIDER = "deepseek-official";

/** Spawn args for ACP stdio — profile only; model is set via session config. */
export function deepseekSpawnArgs(): string[] {
  return ["--profile", "acp"];
}

export function deepseekSessionNewParams(cwd: string): {
  cwd: string;
  mcpServers: [];
} {
  return { cwd, mcpServers: [] };
}

/** ACP prompt blocks: text only (dsh-acp advertises image: false). */
export function deepseekPromptBlocks(text: string): PromptContentBlock[] {
  const trimmed = text.trim();
  return trimmed ? [{ type: "text", text: trimmed }] : [];
}

/**
 * Map a MonoCode native model id to the dsh config option value.
 * dsh uses JSON-array strings like `["deepseek-official","deepseek-v4-flash"]`.
 */
export function deepseekModelConfigValue(nativeId: string): string {
  const trimmed = nativeId.trim();
  if (!trimmed) {
    return JSON.stringify([DEFAULT_PROVIDER, "deepseek-v4-flash"]);
  }
  if (trimmed.startsWith("[")) {
    try {
      const parsed = JSON.parse(trimmed);
      if (Array.isArray(parsed) && parsed.length >= 2) {
        return JSON.stringify(parsed);
      }
    } catch {
      // fall through
    }
  }
  return JSON.stringify([DEFAULT_PROVIDER, trimmed]);
}

/** Extract the short model slug from a dsh config value or native id. */
export function deepseekNativeFromConfigValue(value: string): string {
  const trimmed = value.trim();
  if (!trimmed) return "";
  if (trimmed.startsWith("[")) {
    try {
      const parsed = JSON.parse(trimmed);
      if (Array.isArray(parsed) && parsed.length >= 2) {
        return String(parsed[parsed.length - 1] ?? "").trim();
      }
    } catch {
      // fall through
    }
  }
  return trimmed;
}

export function deepseekEffort(
  modelSettings?: Record<string, string>,
): string | undefined {
  const raw =
    modelSettings?.effort?.trim() ||
    modelSettings?.reasoning?.trim() ||
    modelSettings?.reasoning_effort?.trim();
  if (!raw) return undefined;
  const lower = raw.toLowerCase();
  if (lower === "off" || lower === "none" || lower === "minimal") return "off";
  if (lower === "low" || lower === "medium") return "low";
  if (lower === "max") return "max";
  if (lower === "high" || lower === "xhigh") return "high";
  return raw;
}

export function autoPermissionOption(
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

export function permissionOptionId(
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
      "deny",
    ]) ?? "reject-once"
  );
}

export function permissionRequestFromAcp(
  params: unknown,
): DeepseekPermissionRequest {
  const rec = asRecord(params);
  const subject = asRecord(rec?.subject);
  const tool =
    asRecord(rec?.toolCall) ??
    asRecord(rec?.tool_call) ??
    asRecord(subject?.toolCall) ??
    asRecord(subject) ??
    rec ??
    {};
  const command = stringField(subject ?? {}, "command");
  const kind = stringField(tool, "kind") ?? stringField(subject ?? {}, "kind");
  const preview = extractToolPreview(tool, tool);
  const label =
    toolLabel(tool) ?? command ?? stringField(rec ?? {}, "title");
  const title =
    composeToolTitle({
      kind,
      title: label,
      command: command ?? extractShellCommand(tool),
      skill: extractSkillName(tool),
      path: preview?.path,
      query: preview?.query ?? extractSearchQuery(tool),
      previewKind: preview?.kind,
    }) || "Permission";
  const options = Array.isArray(rec?.options) ? rec.options : [];
  const optionIds = options
    .map((item) => asRecord(item)?.optionId ?? asRecord(item)?.option_id)
    .filter((value): value is string => typeof value === "string");

  return {
    title,
    kind,
    callId:
      stringField(tool, "toolCallId") ??
      stringField(tool, "tool_call_id") ??
      stringField(rec ?? {}, "toolCallId") ??
      stringField(subject ?? {}, "toolCallId"),
    preview,
    optionIds,
  };
}

export function eventsFromAcpUpdate(params: unknown): HarnessEvent[] {
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

  if (
    kind === "tool_call" ||
    kind === "tool_call_update" ||
    kind === "tool_call_content_chunk"
  ) {
    const tool =
      asRecord(update.toolCall) ?? asRecord(update.tool_call) ?? update;
    const callId = String(
      tool.toolCallId ??
        tool.tool_call_id ??
        update.toolCallId ??
        update.tool_call_id ??
        "",
    );
    if (!callId) return [];
    const status = stringField(update, "status") ?? stringField(tool, "status");
    const toolKind =
      stringField(update, "kind") ?? stringField(tool, "kind");
    const preview = extractToolPreview(update, tool);
    const title =
      composeToolTitle({
        kind: toolKind,
        title: toolLabel(update) ?? toolLabel(tool),
        command: extractShellCommand(
          update.rawInput,
          tool.rawInput,
          update.raw_input,
          tool.raw_input,
          update.input,
          tool.input,
          update,
        ),
        skill: extractSkillName(
          update.rawInput,
          tool.rawInput,
          update.raw_input,
          tool.raw_input,
          update.input,
          tool.input,
          update,
        ),
        path: preview?.path,
        query:
          preview?.query ??
          extractSearchQuery(
            update.rawInput ??
              tool.rawInput ??
              update.raw_input ??
              tool.raw_input ??
              update.input ??
              tool.input,
          ),
        previewKind: preview?.kind,
      }) ||
      toolLabel(update) ||
      toolLabel(tool);
    return [
      {
        type: "tool.updated",
        callId,
        title,
        kind: toolKind,
        status,
        detail: toolDetail(update, tool),
        preview,
        ...acpAgentInfo(update, tool, toolKind, title),
      },
    ];
  }

  if (kind === "plan" || kind === "current_plan") {
    const event = planEvent(update);
    return event ? [event] : [];
  }

  return usageFromUpdate(update);
}

export function readConfigOptions(raw: unknown): SessionConfigOption[] {
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

export function extractModelConfigId(options: SessionConfigOption[]): string {
  const exact = options.find((option) => option.id === "model");
  if (exact) return exact.id;
  const model = options.find(
    (option) => option.category === "model" && option.id !== "provider",
  );
  return model?.id ?? "model";
}

export function resolveSettingConfigId(
  options: SessionConfigOption[],
  settingId: string,
): string | undefined {
  const needle = settingId.trim().toLowerCase();
  const exact = options.find((option) => option.id.toLowerCase() === needle);
  if (exact) return exact.id;
  if (
    needle === "effort" ||
    needle === "reasoning" ||
    needle === "reasoning_effort"
  ) {
    return options.find(
      (option) =>
        option.id === "effort" ||
        option.id === "reasoning" ||
        option.id === "reasoning_effort" ||
        option.category === "thought_level",
    )?.id;
  }
  return undefined;
}

export function sessionIdFromResult(result: unknown): string | undefined {
  const rec = asRecord(result);
  const id = rec?.sessionId ?? rec?.session_id ?? rec?.id;
  return typeof id === "string" && id.trim() ? id.trim() : undefined;
}

export function modelsFromSessionConfig(raw: unknown): AgentModel[] {
  if (!Array.isArray(raw)) return [];
  const effort = effortSettingFromConfig(raw);
  const models: AgentModel[] = [];
  for (const item of raw) {
    const rec = asRecord(item);
    if (!rec) continue;
    const id = String(rec.id ?? "").trim();
    if (id !== "model" && rec.category !== "model") continue;
    const groups = Array.isArray(rec.options) ? rec.options : [];
    for (const group of groups) {
      const groupRec = asRecord(group);
      const nested = Array.isArray(groupRec?.options)
        ? groupRec.options
        : Array.isArray(group)
          ? group
          : groupRec
            ? [groupRec]
            : [];
      for (const option of nested) {
        const opt = asRecord(option);
        if (!opt) continue;
        const value = String(opt.value ?? "").trim();
        if (!value) continue;
        const nativeId = deepseekNativeFromConfigValue(value);
        if (!nativeId) continue;
        const name = String(opt.name ?? "").trim() || displayName(nativeId);
        models.push({
          id: `deepseek:${nativeId}`,
          harness: "deepseek",
          name,
          nativeId,
          ...(effort ? { settings: [effort] } : {}),
        });
      }
    }
  }
  return uniqueDeepseekModels(models);
}

export type DeepseekOfficialModelRow = {
  id: string;
  name?: string | null;
  effortLevels?: string[] | null;
  effortDefault?: string | null;
  contextWindow?: number | null;
};

/** Map official `/models` rows into MonoCode AgentModel entries. */
export function modelsFromOfficialApi(
  rows: DeepseekOfficialModelRow[],
): AgentModel[] {
  const models: AgentModel[] = [];
  for (const row of rows) {
    const nativeId = String(row.id ?? "").trim();
    if (!nativeId) continue;
    const name =
      String(row.name ?? "").trim() || displayName(nativeId);
    const effort = effortSettingFromOfficial(row);
    models.push({
      id: `deepseek:${nativeId}`,
      harness: "deepseek",
      name,
      nativeId,
      ...(effort ? { settings: [effort] } : {}),
      ...(typeof row.contextWindow === "number" &&
      Number.isFinite(row.contextWindow) &&
      row.contextWindow > 0
        ? { contextWindow: row.contextWindow }
        : {}),
    });
  }
  return uniqueDeepseekModels(models);
}

function effortSettingFromOfficial(
  row: DeepseekOfficialModelRow,
): ModelSetting | null {
  const levels = (row.effortLevels ?? [])
    .map((level) => String(level).trim().toLowerCase())
    .filter(Boolean);
  if (levels.length === 0) return null;
  const choices = levels.map((value) => ({
    value,
    label: EFFORT_LABELS[value] || value,
  }));
  const preferred = String(row.effortDefault ?? "")
    .trim()
    .toLowerCase();
  const value =
    (preferred && levels.includes(preferred) ? preferred : null) ||
    (levels.includes("high") ? "high" : levels[0]!);
  return {
    id: "effort",
    label: "Reasoning",
    kind: "select",
    value,
    options: choices,
  };
}

export function fallbackDeepseekModels(): AgentModel[] {

  const effort: ModelSetting = {
    id: "effort",
    label: "Reasoning",
    kind: "select",
    value: "high",
    options: [
      { value: "off", label: "Off" },
      { value: "low", label: "Low" },
      { value: "high", label: "High" },
    ],
  };
  return [
    {
      id: "deepseek:deepseek-v4-flash",
      harness: "deepseek",
      name: "DeepSeek V4 Flash",
      nativeId: "deepseek-v4-flash",
      settings: [effort],
    },
    {
      id: "deepseek:deepseek-v4-pro",
      harness: "deepseek",
      name: "DeepSeek V4 Pro",
      nativeId: "deepseek-v4-pro",
      settings: [effort],
    },
  ];
}

export function deepseekStartupError(error: unknown): Error {
  const detail = error instanceof Error ? error.message : String(error);
  if (/auth|login|credential|grant|unauthorized|401|403/i.test(detail)) {
    return new Error(`${detail.trim()}\n\n${translate(AUTH_HELP)}`);
  }
  if (/timed out/i.test(detail)) {
    return new Error(
      `${translate("dsh did not answer initialize in time.")} ${translate(AUTH_HELP)}`,
    );
  }
  if (/-32602|protocolVersion|Invalid params/i.test(detail)) {
    return new Error(
      `dsh ACP rejected initialize (protocolVersion must be a number). ${detail}`,
    );
  }
  return new Error(`DeepSeek Harness (dsh) did not start. ${detail}`);
}

function effortSettingFromConfig(raw: unknown[]): ModelSetting | null {
  for (const item of raw) {
    const rec = asRecord(item);
    if (!rec) continue;
    const id = String(rec.id ?? "").trim();
    if (
      id !== "reasoning_effort" &&
      id !== "effort" &&
      rec.category !== "thought_level"
    ) {
      continue;
    }
    const options = Array.isArray(rec.options) ? rec.options : [];
    const choices: ModelSettingChoice[] = options.flatMap((opt) => {
      const o = asRecord(opt);
      const value = String(o?.value ?? "").trim();
      if (!value) return [];
      return [
        {
          value,
          label: String(o?.name ?? "").trim() || EFFORT_LABELS[value] || value,
        },
      ];
    });
    if (choices.length === 0) continue;
    const current =
      typeof rec.currentValue === "string" ? rec.currentValue : choices[0]?.value;
    return {
      id: "effort",
      label: "Reasoning",
      kind: "select",
      value: current || "high",
      options: choices,
    };
  }
  return null;
}

function uniqueDeepseekModels(models: AgentModel[]): AgentModel[] {
  const seen = new Set<string>();
  const out: AgentModel[] = [];
  for (const model of models) {
    if (seen.has(model.id)) continue;
    seen.add(model.id);
    out.push(model);
  }
  return out;
}

function displayName(nativeId: string): string {
  return nativeId
    .replace(/^deepseek[-_]?/i, "DeepSeek ")
    .replace(/[-_]+/g, " ")
    .replace(/\b\w/g, (ch) => ch.toUpperCase())
    .trim();
}

function usageFromUpdate(update: Record<string, unknown>): HarnessEvent[] {
  const usage =
    asRecord(update.usage) ??
    asRecord(update.tokenUsage) ??
    asRecord(update.token_usage) ??
    (hasUsageFields(update) ? update : null);
  if (!usage) return [];
  const used =
    numberField(usage, "used") ??
    numberField(usage, "usedTokens") ??
    numberField(usage, "used_tokens") ??
    sumNumbers(usage, [
      "inputTokens",
      "outputTokens",
      "input_tokens",
      "output_tokens",
    ]);
  const window =
    numberField(usage, "window") ??
    numberField(usage, "size") ??
    numberField(usage, "contextWindow") ??
    numberField(usage, "context_window") ??
    numberField(usage, "maxTokens") ??
    numberField(usage, "max_tokens");
  const events: HarnessEvent[] = [];
  if (used != null || window != null) {
    events.push({
      type: "context",
      ...(used != null ? { used } : {}),
      ...(window != null ? { window } : {}),
    });
  }
  const inputTokens =
    numberField(usage, "inputTokens") ?? numberField(usage, "input_tokens");
  const outputTokens =
    numberField(usage, "outputTokens") ?? numberField(usage, "output_tokens");
  if (inputTokens != null || outputTokens != null) {
    events.push({
      type: "turn.metrics",
      ...(inputTokens != null ? { inputTokens } : {}),
      ...(outputTokens != null ? { outputTokens } : {}),
    });
  }
  return events;
}

function hasUsageFields(rec: Record<string, unknown>): boolean {
  return (
    numberField(rec, "used") != null ||
    numberField(rec, "usedTokens") != null ||
    numberField(rec, "inputTokens") != null
  );
}

function planEvent(update: Record<string, unknown>): HarnessEvent | null {
  const entries = update.entries ?? update.plan;
  if (Array.isArray(entries)) {
    const items = entries.flatMap((item) => {
      const rec = asRecord(item);
      if (!rec) return [];
      const content = String(rec.content ?? rec.text ?? rec.title ?? "").trim();
      if (!content) return [];
      return [
        {
          text: content,
          status: normalizeTaskListStatus(rec.status),
        },
      ];
    });
    return { type: "tasks.updated", items };
  }
  if (typeof update.text === "string" && update.text.trim()) {
    return { type: "plan", text: update.text };
  }
  return null;
}

function toolLabel(rec: Record<string, unknown>): string | undefined {
  return (
    humanField(rec, "title") ??
    humanField(rec, "name") ??
    humanField(rec, "toolName") ??
    humanField(rec, "tool_name")
  );
}

function toolDetail(
  update: Record<string, unknown>,
  tool: Record<string, unknown>,
): string | undefined {
  const content =
    textFromContent(update.content, "\n") ||
    textFromContent(tool.content, "\n");
  if (content.trim()) return cap(content);
  const output = update.rawOutput ?? tool.rawOutput;
  if (typeof output === "string" && output.trim()) return cap(output);
  const outputText = textFromContent(output);
  return outputText.trim() ? cap(outputText) : undefined;
}

function cap(value: string, max = 8_000): string {
  const text = value.trim();
  if (text.length <= max) return text;
  return `${text.slice(0, max)}\n…`;
}

function pickOption(optionIds: string[], preferred: string[]): string | null {
  for (const id of preferred) {
    if (optionIds.includes(id)) return id;
  }
  return null;
}

function humanField(
  rec: Record<string, unknown>,
  key: string,
): string | undefined {
  const value = stringField(rec, key);
  if (!value || looksLikeCallId(value)) return undefined;
  return value;
}

function looksLikeCallId(value: string): boolean {
  const text = value.trim();
  return (
    /^(call[-_]?|tool[-_])[a-z0-9_-]+$/i.test(text) ||
    /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(
      text,
    )
  );
}

function textFromContent(content: unknown, separator = ""): string {
  if (typeof content === "string") return content;
  const rec = asRecord(content);
  if (rec && typeof rec.text === "string") return rec.text;
  if (rec && rec.content != null) return textFromContent(rec.content, separator);
  if (Array.isArray(content)) {
    return content
      .map((item) => textFromContent(item, separator))
      .filter(Boolean)
      .join(separator);
  }
  return "";
}

export function asRecord(value: unknown): Record<string, unknown> | null {
  if (value && typeof value === "object" && !Array.isArray(value)) {
    return value as Record<string, unknown>;
  }
  return null;
}

export function stringField(
  rec: Record<string, unknown>,
  key: string,
): string | undefined {
  const value = rec[key];
  return typeof value === "string" && value.trim() ? value : undefined;
}

function numberField(
  rec: Record<string, unknown>,
  key: string,
): number | undefined {
  const value = rec[key];
  return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}

function sumNumbers(
  rec: Record<string, unknown>,
  keys: string[],
): number | undefined {
  let total = 0;
  let found = false;
  for (const key of keys) {
    const value = numberField(rec, key);
    if (value == null) continue;
    total += value;
    found = true;
  }
  return found ? total : undefined;
}
