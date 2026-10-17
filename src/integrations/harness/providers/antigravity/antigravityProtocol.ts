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
  /** Select values the option advertises, when it lists them. */
  choices?: string[];
};

// ACP tool updates are patches: results may precede a final status-only update
// and streamed content chunks append rather than replace. Retain only bounded
// output, kind and title by provider-session/call ID, clearing settled entries.
const toolOutputs = new Map<string, { kind?: string; output?: string; title?: string }>();
const MAX_TRACKED_TOOL_OUTPUTS = 256;

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
    const stateKey = JSON.stringify([
      stringField(rec ?? {}, "sessionId") ?? "", callId,
    ]);
    const previous = toolOutputs.get(stateKey);
    const status = stringField(update, "status") ?? stringField(tool, "status");
    const toolKind = stringField(update, "kind") ?? stringField(tool, "kind") ?? previous?.kind;
    const preview = extractToolPreview(update, tool);
    const result = toolDetail(update, tool, toolKind);
    const detail = result && kind === "tool_call_content_chunk" && previous?.output
      ? cap(`${previous.output}\n${result}`)
      : result ?? (status === "completed" || status === "failed" || status === "cancelled"
        ? previous?.output : undefined);
    // The transcript marks successful output explicitly. Keep it in the
    // preview as well as detail: ACP often sends content while in_progress,
    // then a status-only completed update. That last update must not make the
    // earlier result look like request input.
    const outputPreview = detail
      ? preview
        ? { ...preview, output: detail }
        : toolKind === "execute" || toolKind === "shell"
          ? { kind: "shell" as const, output: detail }
          : toolKind === "read"
            ? { kind: "read" as const, output: detail }
            : undefined
      : preview;
    const title =
      composeToolTitle({
        kind: toolKind,
        title: toolLabel(update) ?? toolLabel(tool) ?? previous?.title,
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
    if (status === "completed" || status === "failed" || status === "cancelled") {
      toolOutputs.delete(stateKey);
    } else if (toolKind || detail || title) {
      toolOutputs.delete(stateKey);
      toolOutputs.set(stateKey, { kind: toolKind, output: detail ?? previous?.output, title });
      if (toolOutputs.size > MAX_TRACKED_TOOL_OUTPUTS) {
        toolOutputs.delete(toolOutputs.keys().next().value!);
      }
    }
    return [
      {
        type: "tool.updated",
        callId,
        title,
        kind: toolKind,
        status,
        detail,
        preview: outputPreview,
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
    const choices = configChoices(rec?.options).map((choice) => choice.value);
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
        ...(choices.length > 0 ? { choices } : {}),
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
  kind?: string,
): string | undefined {
  // ACP rawInput and input are invocation arguments, never a tool result.
  // rawOutput may contain arbitrary JSON (including arrays of command results),
  // while content is a collection of user-visible ACP output blocks.
  for (const value of [
    update.rawOutput, tool.rawOutput,
    update.raw_output, tool.raw_output,
    update.output, tool.output,
    update.result, tool.result,
  ]) {
    const text = toolOutputText(value, isCommandKind(kind));
    if (text) return cap(text);
  }
  const content = toolContentText(update.content) || toolContentText(tool.content);
  if (!content) return undefined;
  return cap(isCommandKind(kind) ? commandEnvelopeJson(content) ?? content : content);
}

function isCommandKind(kind?: string): boolean {
  return kind === "execute" || kind === "shell" || kind === "bash";
}

function toolOutputText(output: unknown, command: boolean): string | undefined {
  if (output == null) return undefined;
  if (command) {
    const envelope = commandEnvelopeJson(output);
    if (envelope) return envelope;
  }
  if (typeof output === "string") return output.trim() || undefined;
  if (typeof output === "number" || typeof output === "boolean") return String(output);
  const rec = asRecord(output);
  if (rec) {
    // MCP-shaped results and ACP content wrappers carry readable text inside
    // `content`; prefer that to serializing their entire transport envelope.
    const content = toolContentText(rec.content);
    if (content) return command ? commandEnvelopeJson(content) ?? content : content;
    for (const key of ["output", "result"]) {
      if (rec[key] != null) {
        const nested = toolOutputText(rec[key], command);
        if (nested) return nested;
      }
    }
  }
  // Structured command output (stdout/stderr/exit code), read results, and
  // other provider-specific objects must remain inspectable as formatted JSON.
  try {
    return JSON.stringify(output, null, 2) || undefined;
  } catch {
    return undefined;
  }
}

/** Normalize only explicit ACP *result* envelopes into complete JSON that the
 * transcript can show as command/cwd/exit/output with an optional raw view.
 * Unrecognized JSON and read-file text remain unchanged. */
function commandEnvelopeJson(output: unknown): string | undefined {
  const envelope = commandEnvelope(output);
  if (!envelope) return undefined;
  return boundedCommandJson(envelope);
}

function commandEnvelope(value: unknown, depth = 0): unknown {
  if (depth > 4) return undefined;
  if (typeof value === "string") {
    const text = value.trim();
    if (!text.startsWith("{") && !text.startsWith("[")) return undefined;
    try {
      return commandEnvelope(JSON.parse(text) as unknown, depth + 1);
    } catch {
      return undefined;
    }
  }
  if (Array.isArray(value)) {
    return value.length && value.every(isCommandResult) ? { results: value } : undefined;
  }
  const rec = asRecord(value);
  if (!rec) return undefined;
  if (isCommandResult(value)) return value;
  const results = rec.results;
  if (Array.isArray(results) && results.length && results.every(isCommandResult)) {
    return value;
  }
  if (results != null) {
    const nested = commandEnvelope(results, depth + 1);
    if (nested) {
      const items = asRecord(nested)?.results;
      return { ...rec, results: items ?? [nested] };
    }
  }
  for (const key of ["output", "result"]) {
    if (rec[key] == null) continue;
    const nested = commandEnvelope(rec[key], depth + 1);
    if (nested) return nested;
  }
  return undefined;
}

function isCommandResult(value: unknown): boolean {
  const rec = asRecord(value);
  return !!rec && ["formatted_output", "formattedOutput", "combinedOutput",
    "combined_output", "stdout", "stderr", "stdOut", "stdErr", "exitCode",
    "exit_code"].some((field) => rec[field] != null);
}

/** Cap the output fields *inside* known command JSON. Capping the serialized
 * result with cap() slices quoted JSON mid-string, preventing the UI from
 * recognising command metadata and displaying raw JSON. */
function boundedCommandJson(value: unknown): string | undefined {
  let json: string;
  try {
    json = JSON.stringify(value);
  } catch {
    return undefined;
  }
  if (json.length <= 8_000) return json;
  const data = JSON.parse(json) as unknown;
  const fields: Array<{ record: Record<string, unknown>; key: string; original: string }> = [];
  const outputKeys = ["stdout", "stdOut", "stderr", "stdErr", "combinedOutput",
    "combined_output", "formattedOutput", "formatted_output"];
  const visit = (item: unknown) => {
    const rec = asRecord(item);
    if (!rec) return;
    if (Array.isArray(rec.results)) for (const entry of rec.results) visit(entry);
    for (const key of outputKeys) {
      if (typeof rec[key] === "string") {
        fields.push({ record: rec, key, original: rec[key] as string });
      }
    }
  };
  visit(data);
  // Preserve command/cwd/exit metadata and as much canonical combined output
  // as possible. Trim the longest fields first so an enormous later result
  // does not erase the small, complete earlier commands. On ties prefer
  // source stdout/stderr over the combined/formatted output shown by the UI.
  fields.sort((a, b) => b.original.length - a.original.length ||
    outputKeys.indexOf(a.key) - outputKeys.indexOf(b.key));
  for (const field of fields) {
    if (json.length <= 8_000) break;
    if (!field.original.length) continue;
    field.record[field.key] = "…";
    json = JSON.stringify(data);
    if (json.length > 8_000) continue;
    // Restore the largest prefix fitting into one valid JSON string value.
    let low = 0;
    let high = field.original.length;
    while (low < high) {
      const mid = Math.ceil((low + high) / 2);
      field.record[field.key] = `${field.original.slice(0, mid)}\n…`;
      if (JSON.stringify(data).length <= 8_000) low = mid;
      else high = mid - 1;
    }
    field.record[field.key] = `${field.original.slice(0, low)}\n…`;
    json = JSON.stringify(data);
    if (json.length > 8_000) {
      field.record[field.key] = "…";
      json = JSON.stringify(data);
    }
  }
  return json.length <= 8_000 ? json : undefined;
}

function toolContentText(content: unknown): string | undefined {
  if (typeof content === "string") return content.trim() || undefined;
  if (Array.isArray(content)) {
    const parts = content.map(toolContentText).filter((part): part is string => !!part);
    return parts.length ? parts.join("\n") : undefined;
  }
  const rec = asRecord(content);
  if (!rec) return undefined;
  // `diff` and `terminal` are metadata, not readable tool output. In ACP v1
  // content is {type:"content",content:{type:"text",text:"..."}}.
  if (rec.type === "content") return toolContentText(rec.content);
  if (rec.type === "text") return stringField(rec, "text");
  if (rec.type === "resource") return toolContentText(rec.resource);
  if (rec.type === "resource_link") return undefined;
  if (rec.type == null) {
    if (typeof rec.text === "string") return rec.text.trim() || undefined;
    if (rec.content != null) return toolContentText(rec.content);
  }
  return undefined;
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
