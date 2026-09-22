import { promptBlocks, type PromptContentBlock } from "../../../../features/sessions/model/attachments";
import type { AgentModel, ModelSetting, ModelSettingChoice } from "../../../../features/sessions/model/models";
import type { Attachment, RuntimeMode, ToolPreview } from "../../../../features/sessions/model/session";
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

export type AntigravityModeId = "default" | "auto_edit" | "yolo";

export type AntigravityPermissionRequest = {
  title: string;
  kind?: string;
  callId?: string;
  preview?: ToolPreview;
  optionIds: string[];
  optionKinds: Record<string, string>;
};

export type SessionConfigOption = {
  id: string;
  category?: string;
  type?: string;
  currentValue?: string | boolean;
};

export function antigravityPromptBlocks(
  text: string,
  attachments: Attachment[] = [],
): PromptContentBlock[] {
  return promptBlocks(text, attachments);
}

/** The raw .par locates sibling resources relative to its process directory. */
export function antigravitySpawnCwd(binary: string, fallback: string): string {
  const separator = Math.max(binary.lastIndexOf("/"), binary.lastIndexOf("\\"));
  return separator >= 0 ? binary.slice(0, separator + 1) : fallback;
}

export function antigravityModeId(
  runtimeMode: RuntimeMode,
  planning = false,
): AntigravityModeId {
  // No native plan mode. Use default and deny non-read permissions on the client.
  if (planning) return "default";
  if (runtimeMode === "full-access") return "yolo";
  if (runtimeMode === "auto-accept-edits") return "auto_edit";
  return "default";
}

export function autoPermissionOption(
  runtimeMode: RuntimeMode,
  kind: string | undefined,
  optionIds: string[],
  optionKinds: Record<string, string> = {},
): string | null {
  if (
    runtimeMode !== "full-access" &&
    !(runtimeMode === "auto-accept-edits" && kind === "edit")
  ) return null;
  return permissionOptionId("allow", optionIds, optionKinds);
}

/** ACP option IDs are opaque; prefer the advertised semantic kind. */
export function permissionOptionId(
  decision: ApprovalDecision,
  optionIds: string[],
  optionKinds: Record<string, string> = {},
): string | null {
  const kinds = decision === "allow"
    ? ["allow_once", "allow_always"]
    : ["reject_once", "reject_always"];
  for (const kind of kinds) {
    const id = optionIds.find((id) => optionKinds[id] === kind);
    if (id) return id;
  }
  return pickOption(optionIds, decision === "allow"
    ? ["allow-once", "allow_once", "allow-always", "allow_always", "allow"]
    : ["reject-once", "reject_once", "reject-always", "reject_always", "reject", "deny"]);
}

export function permissionRequestFromAcp(
  params: unknown,
): AntigravityPermissionRequest {
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
    toolLabel(tool) ??
    command ??
    stringField(rec ?? {}, "title");
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
  const optionKinds: Record<string, string> = {};
  for (const item of options) {
    const option = asRecord(item);
    const id = option?.optionId ?? option?.option_id;
    if (typeof id === "string" && typeof option?.kind === "string") {
      optionKinds[id] = option.kind;
    }
  }

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
    optionKinds,
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
    const toolKind = stringField(update, "kind") ?? stringField(tool, "kind");
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

/** Discover models and reasoning controls from standard ACP session config. */
export function modelsFromSessionNew(raw: unknown): AgentModel[] {
  const rec = asRecord(raw);
  const options = Array.isArray(rec?.configOptions) ? rec.configOptions : [];
  const modelId = extractModelConfigId(readConfigOptions(options));
  const modelOption = options.map(asRecord).find((item) => item?.id === modelId);
  const choices = configChoices(modelOption?.options);
  const settings: ModelSetting[] = [];
  for (const item of options) {
    const option = asRecord(item);
    if (option?.category !== "thought_level") continue;
    const values = configChoices(option.options);
    if (values.length < 2) continue;
    const current = String(option.currentValue ?? "");
    settings.push({
      id: "effort",
      label: "Effort",
      kind: "select",
      value: values.some((choice) => choice.value === current)
        ? current : values[0].value,
      options: values,
    });
  }
  const legacy = asRecord(rec?.models);
  const available = Array.isArray(legacy?.availableModels)
    ? legacy.availableModels : [];
  const models = choices.length > 0 ? choices : available.flatMap((item) => {
    const model = asRecord(item);
    const value = stringField(model ?? {}, "modelId");
    return value ? [{ value, label: String(model?.name ?? value) }] : [];
  });
  return [...new Map(models.map(({ value, label }) => [value, {
    id: `antigravity:${value}`,
    harness: "antigravity" as const,
    name: label,
    nativeId: value,
    ...(settings.length > 0 ? { settings } : {}),
  }])).values()];
}

function configChoices(raw: unknown): ModelSettingChoice[] {
  if (!Array.isArray(raw)) return [];
  return raw.flatMap((item) => {
    const rec = asRecord(item);
    if (Array.isArray(rec?.options)) return configChoices(rec.options);
    const value = stringField(rec ?? {}, "value");
    return value ? [{ value, label: String(rec?.name ?? rec?.label ?? value) }] : [];
  });
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
        type: typeof rec?.type === "string" ? rec.type : undefined,
        currentValue:
          typeof rec?.currentValue === "string" ||
          typeof rec?.currentValue === "boolean"
            ? rec.currentValue
            : undefined,
      },
    ];
  });
}

export function extractModelConfigId(
  options: SessionConfigOption[],
): string {
  // A model selector is never a boolean option — a boolean typed "model" would
  // coerce any model id to false in set_config_option.
  const isSelectable = (option: SessionConfigOption) =>
    option.type !== "boolean";
  const exact = options.find(
    (option) => option.id === "model" && isSelectable(option),
  );
  if (exact) return exact.id;
  // A provider selector is not the model picker.
  const model = options.find(
    (option) =>
      option.category === "model" &&
      option.id !== "provider" &&
      isSelectable(option),
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
  if (needle === "effort" || needle === "reasoning") {
    return options.find(
      (option) =>
        option.id === "effort" ||
        option.id === "reasoning" ||
        option.category === "thought_level",
    )?.id;
  }
  if (needle === "fast" || needle === "fastmode" || needle === "fast_mode") {
    return options.find(
      (option) =>
        option.id === "fast" ||
        option.id === "fast_mode" ||
        option.id.toLowerCase().includes("fast"),
    )?.id;
  }
  return undefined;
}

export function sessionIdFromResult(result: unknown): string | undefined {
  const rec = asRecord(result);
  const id = rec?.sessionId ?? rec?.session_id ?? rec?.id;
  return typeof id === "string" && id.trim() ? id.trim() : undefined;
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
    sumNumbers(usage, ["inputTokens", "outputTokens", "input_tokens", "output_tokens"]);
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
  const cacheReadTokens =
    numberField(usage, "cacheReadTokens") ??
    numberField(usage, "cache_read_input_tokens");
  const cacheWriteTokens =
    numberField(usage, "cacheWriteTokens") ??
    numberField(usage, "cache_creation_input_tokens");
  const cacheReported = cacheReadTokens != null || cacheWriteTokens != null;
  if (
    inputTokens != null ||
    outputTokens != null ||
    cacheReadTokens != null ||
    cacheWriteTokens != null
  ) {
    const cacheableInput =
      (inputTokens ?? 0) + (cacheReadTokens ?? 0) + (cacheWriteTokens ?? 0);
    events.push({
      type: "turn.metrics",
      ...(inputTokens != null ? { inputTokens } : {}),
      ...(outputTokens != null ? { outputTokens } : {}),
      ...(cacheReadTokens != null ? { cacheReadTokens } : {}),
      ...(cacheWriteTokens != null ? { cacheWriteTokens } : {}),
      ...(cacheReported && cacheableInput > 0
        ? { cacheHitPercent: ((cacheReadTokens ?? 0) / cacheableInput) * 100 }
        : {}),
    });
  }
  return events;
}

function hasUsageFields(rec: Record<string, unknown>): boolean {
  return [
    "used",
    "usedTokens",
    "used_tokens",
    "inputTokens",
    "input_tokens",
    "outputTokens",
    "output_tokens",
    "cacheReadTokens",
    "cache_read_input_tokens",
    "cacheWriteTokens",
    "cache_creation_input_tokens",
    "window",
    "size",
    "contextWindow",
    "context_window",
    "maxTokens",
    "max_tokens",
  ].some((key) => numberField(rec, key) != null);
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
