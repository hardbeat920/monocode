import type { PromptContentBlock } from "../attachments";
import type { AgentModel, ModelSetting, ModelSettingChoice } from "../models";
import type { RuntimeMode, ToolPreview } from "../session";
import { normalizeTaskListStatus } from "../taskList";
import type { ApprovalDecision, HarnessEvent } from "./types";
import {
  composeToolTitle,
  extractSearchQuery,
  extractShellCommand,
  extractSkillName,
  extractToolPreview,
} from "./preview";
import { fxToolInfo, fxToolVerb } from "./fxTool";

export type FxModeId = "ask" | "code";

export type FxPermissionRequest = {
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
  auto: "Auto",
  none: "None",
  minimal: "Minimal",
  low: "Low",
  medium: "Medium",
  high: "High",
  xhigh: "Extra High",
  max: "Max",
};

/** ACP prompt blocks for fx: text only. Image and audio are not accepted. */
export function fxPromptBlocks(text: string): PromptContentBlock[] {
  const trimmed = text.trim();
  return trimmed ? [{ type: "text", text: trimmed }] : [];
}

/**
 * fx always runs in its own `code` (auto) mode.
 *
 * fx defaults to `ask`, which makes it stop and request permission for every
 * read, list and command. We do not surface those prompts, so a turn that
 * touched a single file would park forever with nothing on screen. fx has a
 * working auto mode; use it and let fx police itself.
 */
export function fxModeId(_runtimeMode: RuntimeMode): FxModeId {
  return "code";
}

/**
 * Approve anything fx still asks about. `set_mode` can fail, and fx keeps a few
 * prompts even in code mode, so this is the backstop that guarantees a turn is
 * never blocked on an approval nobody can see.
 */
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
): FxPermissionRequest {
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
  const fx = fxToolInfo(tool, tool);
  const preview = fx.resolved
    ? fx.preview
    : (fx.preview ?? extractToolPreview(tool, tool));
  const label =
    fx.title ??
    fxToolVerb(toolLabel(tool)) ??
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
    // fx sends no rawInput/locations/diff, so the generic ACP extraction has
    // nothing to work with. Mine the result blob instead, and only fall back to
    // the shared path if fx ever starts sending structured fields.
    const fx = fxToolInfo(update, tool);
    const toolKind =
      fx.kind ?? stringField(update, "kind") ?? stringField(tool, "kind");
    const preview = fx.resolved
      ? fx.preview
      : (fx.preview ?? extractToolPreview(update, tool));
    const title =
      composeToolTitle({
        kind: toolKind,
        title: fx.title ?? toolLabel(update) ?? toolLabel(tool),
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
      fx.title ||
      toolLabel(update) ||
      toolLabel(tool);
    return [
      {
        type: "tool.updated",
        callId,
        title,
        kind: toolKind,
        status,
        detail: cap(fx.detail ?? "") || toolDetail(update, tool),
        preview,
      },
    ];
  }

  if (kind === "plan" || kind === "current_plan") {
    const event = planEvent(update);
    return event ? [event] : [];
  }

  const usage = usageFromUpdate(update);
  return usage ? [usage] : [];
}

export function modelsFromFxOutput(stdout: string): AgentModel[] {
  const trimmed = stdout.trim();
  if (!trimmed) return [];
  try {
    return uniqueFxModels(modelsFromFxJson(JSON.parse(trimmed)));
  } catch {
    const start = trimmed.indexOf("{");
    const arrayStart = trimmed.indexOf("[");
    const jsonAt =
      start >= 0 && (arrayStart < 0 || start < arrayStart)
        ? start
        : arrayStart;
    if (jsonAt >= 0) {
      try {
        return uniqueFxModels(modelsFromFxJson(JSON.parse(trimmed.slice(jsonAt))));
      } catch {
        // Fall through to line parsing.
      }
    }
    return uniqueFxModels(modelsFromFxText(trimmed));
  }
}

/** fx can omit its active/TUI-selected model from `models --json`. */
export function modelFromFxStatusOutput(stdout: string): AgentModel | null {
  const trimmed = stdout.trim();
  if (!trimmed) return null;
  let raw: unknown;
  try {
    raw = JSON.parse(trimmed);
  } catch {
    const start = trimmed.indexOf("{");
    if (start < 0) return null;
    try {
      raw = JSON.parse(trimmed.slice(start));
    } catch {
      return null;
    }
  }
  const model = stringField(asRecord(raw) ?? {}, "model");
  return model ? modelFromJson(model) : null;
}

export function mergeFxCatalogModels(
  models: AgentModel[],
  active: AgentModel | null,
): AgentModel[] {
  return uniqueFxModels(active ? [...models, active] : models);
}

export function modelsFromFxJson(raw: unknown): AgentModel[] {
  const rec = asRecord(raw);
  const list = Array.isArray(raw)
    ? raw
    : Array.isArray(rec?.models)
      ? rec.models
      : Array.isArray(rec?.ids)
        ? rec.ids
        : Array.isArray(rec?.data)
          ? rec.data
          : [];
  const models: AgentModel[] = [];
  for (const item of list) {
    const model = modelFromJson(item);
    if (model) models.push(model);
  }
  return models;
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

export function extractModelConfigId(
  options: SessionConfigOption[],
): string {
  const exact = options.find((option) => option.id === "model");
  if (exact) return exact.id;
  // fx lists provider first with category "model"; that is not the model picker.
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

function modelFromJson(item: unknown): AgentModel | null {
  const rec = asRecord(item);
  if (!rec) {
    if (typeof item === "string" && item.trim()) {
      const nativeId = item.trim();
      return {
        id: `fx:${nativeId}`,
        harness: "fx",
        name: nativeId,
        nativeId,
      };
    }
    return null;
  }
  const nativeId = String(
    rec.id ?? rec.modelId ?? rec.model_id ?? rec.model ?? rec.value ?? "",
  ).trim();
  if (!nativeId) return null;
  const name = String(rec.name ?? rec.displayName ?? rec.title ?? "").trim();
  const settings = settingsFromJson(rec);
  const window = numberField(rec, "contextWindow") ??
    numberField(rec, "context_window") ??
    numberField(rec, "window");
  return {
    id: `fx:${nativeId}`,
    harness: "fx",
    name: name || displayName(nativeId),
    nativeId,
    ...(settings.length > 0 ? { settings } : {}),
    ...(window ? { contextWindow: window } : {}),
  };
}

function settingsFromJson(rec: Record<string, unknown>): ModelSetting[] {
  const settings: ModelSetting[] = [];
  const effortOptions = effortChoices(rec);
  if (effortOptions.length > 1) {
    settings.push({
      id: "effort",
      label: "Effort",
      kind: "select",
      value: stringField(rec, "effort") ?? effortOptions[0]?.value ?? "auto",
      options: effortOptions,
    });
  }
  if (rec.fast === true || rec.fast_mode === true || rec.supportsFast === true) {
    settings.push({
      id: "fast",
      label: "Fast",
      kind: "toggle",
      value: rec.fast_mode === true || rec.fast === true ? "true" : "false",
      options: [
        { value: "true", label: "On" },
        { value: "false", label: "Off" },
      ],
    });
  }
  return settings;
}

function effortChoices(rec: Record<string, unknown>): ModelSettingChoice[] {
  const raw =
    rec.effortOptions ??
    rec.effort_options ??
    rec.efforts ??
    rec.supportedEffort;
  const values = Array.isArray(raw)
    ? raw.flatMap((item) => {
        if (typeof item === "string" && item.trim()) return [item.trim()];
        const nested = asRecord(item);
        const value = nested && stringField(nested, "value");
        return value ? [value] : [];
      })
    : [];
  return values.map((value) => ({
    value,
    label: EFFORT_LABELS[value] ?? value,
  }));
}

function modelsFromFxText(stdout: string): AgentModel[] {
  const models: AgentModel[] = [];
  for (const raw of stdout.split(/\r?\n/)) {
    const line = raw.replace(/\x1b\[[0-9;]*[A-Za-z]/g, "").trim();
    const match = /^(\S+)\s+[—-]\s+(.+)$/.exec(line);
    if (!match) continue;
    const nativeId = match[1];
    const name = match[2].replace(/\s*\(default\)\s*$/i, "").trim();
    if (!nativeId || !name) continue;
    models.push({
      id: `fx:${nativeId}`,
      harness: "fx",
      name,
      nativeId,
    });
  }
  return models;
}

function uniqueFxModels(models: AgentModel[]): AgentModel[] {
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
  const slug = nativeId.includes("/")
    ? nativeId.slice(nativeId.indexOf("/") + 1)
    : nativeId;
  return slug
    .replace(/[-_]+/g, " ")
    .replace(/\b\w/g, (ch) => ch.toUpperCase());
}

function usageFromUpdate(update: Record<string, unknown>): HarnessEvent | null {
  const usage =
    asRecord(update.usage) ??
    asRecord(update.tokenUsage) ??
    asRecord(update.token_usage) ??
    (hasUsageFields(update) ? update : null);
  if (!usage) return null;
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
  if (used == null && window == null) return null;
  return { type: "context", used: used ?? undefined, window: window ?? undefined };
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
