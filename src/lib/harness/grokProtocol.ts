import { promptBlocks, type PromptContentBlock } from "../attachments";
import type { AgentModel, ModelSetting, ModelSettingChoice } from "../models";
import type { Attachment, RuntimeMode, ToolPreview } from "../session";
import { normalizeTaskListStatus } from "../taskList";
import type { ApprovalDecision, HarnessEvent } from "./types";
import type { UserQuestion, UserQuestionReply } from "../userQuestion";
import { questionsFromUnknown, selectedAnswerLabels } from "../userQuestion";
import {
  composeToolTitle,
  extractSearchQuery,
  extractShellCommand,
  extractSkillName,
  extractToolPreview,
} from "./preview";

export const AUTH_HELP =
  "Grok Build is not signed in. Run `grok login` in a terminal, or set XAI_API_KEY.";

export const TEXT_MODEL = "grok-4.6";

const VARIANT_KIND: Record<string, string> = {
  readfile: "read",
  read: "read",
  write: "edit",
  edit: "edit",
  searchreplace: "edit",
  bash: "execute",
  execute: "execute",
  run_terminal_command: "execute",
  grep: "search",
  search: "search",
  webfetch: "fetch",
  web_fetch: "fetch",
  websearch: "search",
  web_search: "search",
  listdir: "read",
  list_dir: "read",
  agent: "agent",
  task: "agent",
  subagent: "agent",
};

const EFFORT_LABELS: Record<string, string> = {
  xhigh: "Extra High",
  high: "High",
  medium: "Medium",
  low: "Low",
};

export type GrokPermissionRequest = {
  title: string;
  kind?: string;
  callId?: string;
  preview?: ToolPreview;
  optionIds: string[];
};

export type GrokAskQuestion = UserQuestion;

export function askQuestionsFromAcp(params: unknown): UserQuestion[] {
  return questionsFromUnknown(params);
}

export function askQuestionResponse(
  reply: UserQuestionReply,
  questions: UserQuestion[],
): Record<string, unknown> {
  if (reply.kind !== "answered") return { outcome: "skip_interview" };
  const answers: Record<string, string | string[]> = {};
  for (const question of questions) {
    const labels = selectedAnswerLabels(question, reply);
    if (labels.length === 0) continue;
    answers[question.prompt] = question.multiSelect
      ? labels
      : (labels[0] ?? "");
  }
  return { outcome: "accepted", answers };
}

/** Grok accepts ACP image blocks despite advertising image: false. */
export function grokPromptBlocks(
  text: string,
  attachments: Attachment[] = [],
): PromptContentBlock[] {
  return promptBlocks(text, attachments);
}

export function grokSpawnArgs(input: {
  model: string;
  effort?: string;
  fullAccess?: boolean;
  plan?: boolean;
}): string[] {
  const args = ["--no-auto-update"];
  if (input.plan) args.push("--permission-mode", "plan");
  args.push("agent", "--no-leader");
  const native = nativeId(input.model);
  if (native) args.push("--model", native);
  const effort = input.effort?.trim();
  if (effort) args.push("--reasoning-effort", effort);
  if (input.fullAccess) args.push("--always-approve");
  args.push("stdio");
  return args;
}

export function grokTextSpawnArgs(): string[] {
  return [
    "--no-auto-update",
    "--permission-mode",
    "dontAsk",
    "agent",
    "--no-leader",
    "--model",
    TEXT_MODEL,
    "--reasoning-effort",
    "low",
    "stdio",
  ];
}

export function grokSessionNewParams(
  cwd: string,
  runtimeMode: RuntimeMode,
): Record<string, unknown> {
  const params: Record<string, unknown> = { cwd, mcpServers: [] };
  if (runtimeMode === "full-access") {
    params._meta = { yoloMode: true };
  } else if (runtimeMode === "auto") {
    params._meta = { autoMode: true };
  }
  return params;
}

export function grokEffort(
  settings?: Record<string, string>,
): string | undefined {
  const value = settings?.effort?.trim() || settings?.reasoning?.trim();
  return value || undefined;
}

/**
 * Never pick `grok.com` — that starts a browser OAuth flow with no headless
 * completion path. Prefer an API key when the agent advertised it (it saw
 * XAI_API_KEY), otherwise the cached `grok login` token.
 */
export function grokAuthMethodId(init: unknown): string | null {
  const rec = asRecord(init);
  const methods = Array.isArray(rec?.authMethods) ? rec.authMethods : [];
  const ids = new Set(
    methods.flatMap((item) => {
      const id = asRecord(item)?.id;
      return typeof id === "string" && id.trim() && id !== "grok.com"
        ? [id.trim()]
        : [];
    }),
  );
  const defaultId = stringField(
    asRecord(rec?._meta) ?? {},
    "defaultAuthMethodId",
  );
  if (ids.has("xai.api_key")) return "xai.api_key";
  if (defaultId && ids.has(defaultId)) return defaultId;
  if (ids.has("cached_token")) return "cached_token";
  const first = [...ids][0];
  return first ?? null;
}

export function grokAuthError(error: unknown): Error {
  const detail = error instanceof Error ? error.message : String(error);
  if (/auth|login|credential|api key|XAI_API_KEY/i.test(detail)) {
    return new Error(`${detail.trim()}\n\n${AUTH_HELP}`);
  }
  if (/timed out/i.test(detail)) {
    return new Error(`Grok Build did not start. ${AUTH_HELP}`);
  }
  return new Error(`Grok Build did not start. ${detail}`);
}

export function pickAutoOption(
  runtimeMode: RuntimeMode,
  kind: string | undefined,
  optionIds: string[],
): string | null {
  if (optionIds.length === 0) return null;
  const tool = (kind ?? "").toLowerCase();
  if (runtimeMode === "supervised") return null;
  if (
    runtimeMode === "auto-accept-edits" &&
    (tool === "execute" || tool === "other" || tool === "fetch")
  ) {
    return null;
  }
  if (runtimeMode === "full-access") {
    return pickOption(optionIds, [
      "allow-always",
      "allow_always",
      "allow-once",
      "allow_once",
      "allow",
    ]);
  }
  return pickOption(optionIds, [
    "allow-once",
    "allow_once",
    "allow-always",
    "allow_always",
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
): GrokPermissionRequest {
  const rec = asRecord(params);
  const subject = asRecord(rec?.subject);
  const tool =
    asRecord(rec?.toolCall) ??
    asRecord(rec?.tool_call) ??
    asRecord(subject?.toolCall) ??
    asRecord(subject) ??
    rec ??
    {};
  const grok = grokToolFields(tool, tool);
  const kind =
    grok.kind ??
    stringField(tool, "kind") ??
    stringField(subject ?? {}, "kind");
  const preview = extractToolPreview(tool, tool);
  const command = grok.command ?? extractShellCommand(tool);
  const title =
    composeToolTitle({
      kind,
      title: grok.title ?? toolLabel(tool),
      command,
      skill: extractSkillName(tool),
      path: grok.path ?? preview?.path,
      query: grok.query ?? preview?.query ?? extractSearchQuery(tool),
      previewKind: preview?.kind,
    }) ||
    grok.title ||
    toolLabel(tool) ||
    "Permission";
  const options = Array.isArray(rec?.options) ? rec.options : [];
  const optionIds = options
    .map((item) => asRecord(item)?.optionId ?? asRecord(item)?.option_id)
    .filter((value): value is string => typeof value === "string");

  return {
    title,
    kind,
    callId:
      grok.callId ??
      stringField(tool, "toolCallId") ??
      stringField(tool, "tool_call_id") ??
      stringField(rec ?? {}, "toolCallId"),
    preview: mergePreview(preview, grok.path, grok.query, kind),
    optionIds,
  };
}

export function planFromExitPlan(params: unknown): string {
  const rec = asRecord(params);
  const nested = asRecord(rec?.input);
  const text =
    rec?.planContent ??
    rec?.plan ??
    rec?.content ??
    nested?.plan ??
    nested?.planContent;
  return typeof text === "string" ? text.trim() : "";
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

  if (kind === "tool_call_delta_chunk") {
    const callId =
      stringField(update, "toolCallId") ??
      stringField(update, "tool_call_id") ??
      "";
    if (!callId) return [];
    const name = stringField(update, "name") ?? stringField(update, "title");
    return [
      {
        type: "tool.updated",
        callId,
        title: name ? humanizeToolName(name) : undefined,
        kind: kindFromName(name),
        status: "pending",
      },
    ];
  }

  if (
    kind === "tool_call" ||
    kind === "tool_call_update" ||
    kind === "tool_call_content_chunk"
  ) {
    const tool =
      asRecord(update.toolCall) ?? asRecord(update.tool_call) ?? update;
    const grok = grokToolFields(update, tool);
    const callId =
      grok.callId ??
      String(
        tool.toolCallId ??
          tool.tool_call_id ??
          update.toolCallId ??
          update.tool_call_id ??
          "",
      );
    if (!callId) return [];
    const toolKind =
      grok.kind ?? stringField(update, "kind") ?? stringField(tool, "kind");
    const status = stringField(update, "status") ?? stringField(tool, "status");
    const preview = mergePreview(
      extractToolPreview(update, tool),
      grok.path,
      grok.query,
      toolKind,
    );
    const title =
      composeToolTitle({
        kind: toolKind,
        title: grok.title ?? toolLabel(update) ?? toolLabel(tool),
        command:
          grok.command ??
          extractShellCommand(
            update.rawInput,
            tool.rawInput,
            update.raw_input,
            tool.raw_input,
            update.input,
            tool.input,
            grok.input,
          ),
        skill: extractSkillName(
          update.rawInput,
          tool.rawInput,
          update.raw_input,
          tool.raw_input,
          update.input,
          tool.input,
        ),
        path: preview?.path,
        query: preview?.query ?? grok.query,
        previewKind: preview?.kind,
      }) ||
      grok.title ||
      toolLabel(update) ||
      toolLabel(tool);
    return [
      {
        type: "tool.updated",
        callId,
        title,
        kind: toolKind,
        status,
        detail: cap(toolDetail(update, tool) ?? "") || undefined,
        preview,
      },
    ];
  }

  if (kind === "plan" || kind === "current_plan") {
    const event = planEvent(update);
    return event ? [event] : [];
  }

  if (kind === "session_summary_generated") {
    return [];
  }

  const usage = usageFromUpdate(update);
  return usage ? [usage] : [];
}

export function sessionIdFromResult(result: unknown): string | undefined {
  const rec = asRecord(result);
  const id = rec?.sessionId ?? rec?.session_id ?? rec?.id;
  return typeof id === "string" && id.trim() ? id.trim() : undefined;
}

export function contextWindowFromSetup(result: unknown): number | undefined {
  const models = [
    ...modelsFromSessionNew(result),
    ...modelsFromInitialize(result),
  ];
  const current = currentModelId(result);
  const match = models.find((model) => model.nativeId === current) ?? models[0];
  return match?.contextWindow;
}

export function currentModelId(result: unknown): string | undefined {
  const rec = asRecord(result);
  const models = asRecord(rec?.models);
  const meta = asRecord(rec?._meta);
  const state = asRecord(meta?.modelState);
  return (
    stringField(models ?? {}, "currentModelId") ??
    stringField(state ?? {}, "currentModelId") ??
    stringField(meta ?? {}, "currentModelId")
  );
}

export function modelsFromInitialize(result: unknown): AgentModel[] {
  const rec = asRecord(result);
  const meta = asRecord(rec?._meta);
  const state = asRecord(meta?.modelState);
  return modelsFromAvailable(state?.availableModels ?? rec?.availableModels);
}

export function modelsFromSessionNew(result: unknown): AgentModel[] {
  const rec = asRecord(result);
  const models = asRecord(rec?.models);
  return modelsFromAvailable(
    models?.availableModels ?? asRecord(rec?._meta)?.availableModels,
  );
}

export function modelsFromGrokModelsOutput(stdout: string): AgentModel[] {
  const models: AgentModel[] = [];
  for (const raw of stdout.split(/\r?\n/)) {
    const line = raw.replace(/\x1b\[[0-9;]*[A-Za-z]/g, "").trim();
    const match = /^[*+\-]\s+(\S+)/.exec(line);
    if (!match) continue;
    const nativeId = match[1].trim();
    if (!nativeId) continue;
    models.push(modelFromNative(nativeId, displayName(nativeId)));
  }
  return uniqueGrokModels(models);
}

export function fallbackGrokModels(): AgentModel[] {
  return [
    modelFromNative("grok-4.6", "Grok 4.6", {
      contextWindow: 500_000,
      efforts: [
        { value: "xhigh", label: "Extra High" },
        { value: "high", label: "High", default: true },
        { value: "medium", label: "Medium" },
        { value: "low", label: "Low" },
      ],
    }),
    modelFromNative("grok-4.5", "Grok 4.5", {
      contextWindow: 500_000,
      efforts: [
        { value: "high", label: "High", default: true },
        { value: "medium", label: "Medium" },
        { value: "low", label: "Low" },
      ],
    }),
  ];
}

function modelsFromAvailable(raw: unknown): AgentModel[] {
  if (!Array.isArray(raw)) return [];
  const models: AgentModel[] = [];
  for (const item of raw) {
    const rec = asRecord(item);
    if (!rec) continue;
    const nativeId = String(
      rec.modelId ?? rec.model_id ?? rec.id ?? rec.value ?? "",
    ).trim();
    if (!nativeId) continue;
    const name = String(rec.name ?? rec.displayName ?? nativeId).trim();
    const meta = asRecord(rec._meta) ?? rec;
    const window =
      numberField(meta, "totalContextTokens") ??
      numberField(meta, "contextWindow") ??
      numberField(rec, "contextWindow");
    const efforts = reasoningEfforts(meta);
    models.push(
      modelFromNative(nativeId, name || displayName(nativeId), {
        contextWindow: window,
        efforts,
        defaultEffort: stringField(meta, "reasoningEffort"),
      }),
    );
  }
  return uniqueGrokModels(models);
}

function modelFromNative(
  nativeId: string,
  name: string,
  extra?: {
    contextWindow?: number;
    efforts?: Array<ModelSettingChoice & { default?: boolean }>;
    defaultEffort?: string;
  },
): AgentModel {
  const efforts = extra?.efforts ?? [];
  const settings =
    efforts.length > 0
      ? [
          effortSetting(
            efforts,
            extra?.defaultEffort ??
              efforts.find((item) => item.default)?.value ??
              efforts[0]?.value,
          ),
        ]
      : undefined;
  return {
    id: `grok:${nativeId}`,
    harness: "grok",
    name,
    nativeId,
    ...(settings ? { settings } : {}),
    ...(extra?.contextWindow ? { contextWindow: extra.contextWindow } : {}),
  };
}

function effortSetting(
  options: ModelSettingChoice[],
  value?: string,
): ModelSetting {
  return {
    id: "effort",
    label: "Reasoning",
    kind: "select",
    value:
      value && options.some((item) => item.value === value)
        ? value
        : (options[0]?.value ?? "high"),
    options: options.map((item) => ({
      value: item.value,
      label: EFFORT_LABELS[item.value] ?? item.label,
    })),
  };
}

function reasoningEfforts(
  meta: Record<string, unknown>,
): Array<ModelSettingChoice & { default?: boolean }> {
  const raw = meta.reasoningEfforts ?? meta.reasoning_efforts;
  if (!Array.isArray(raw)) return [];
  return raw.flatMap((item) => {
    const rec = asRecord(item);
    const value = String(rec?.value ?? rec?.id ?? "").trim();
    if (!value) return [];
    const label = String(rec?.label ?? EFFORT_LABELS[value] ?? value)
      .replace(/\s+Effort$/i, "")
      .trim();
    return [
      {
        value,
        label,
        default: rec?.default === true,
      },
    ];
  });
}

function grokToolFields(
  update: Record<string, unknown>,
  tool: Record<string, unknown>,
): {
  kind?: string;
  title?: string;
  path?: string;
  command?: string;
  query?: string;
  callId?: string;
  input?: Record<string, unknown>;
} {
  const meta = nestedMeta(update, "x.ai/tool") ?? nestedMeta(tool, "x.ai/tool");
  const input =
    asRecord(meta?.input) ??
    asRecord(update.rawInput) ??
    asRecord(update.raw_input) ??
    asRecord(tool.rawInput) ??
    asRecord(tool.raw_input) ??
    asRecord(update.input) ??
    asRecord(tool.input);
  const variant = String(input?.variant ?? meta?.name ?? "").toLowerCase();
  return {
    kind:
      stringField(meta ?? {}, "kind") ??
      VARIANT_KIND[variant.replace(/[^a-z0-9]+/g, "")] ??
      VARIANT_KIND[variant],
    title:
      stringField(meta ?? {}, "label") ??
      stringField(update, "title") ??
      stringField(tool, "title"),
    path:
      stringField(input ?? {}, "path") ??
      stringField(input ?? {}, "absolute_path") ??
      stringField(input ?? {}, "file_path"),
    command: stringField(input ?? {}, "command"),
    query:
      stringField(input ?? {}, "query") ??
      stringField(input ?? {}, "pattern") ??
      stringField(input ?? {}, "search"),
    callId:
      stringField(update, "toolCallId") ??
      stringField(update, "tool_call_id") ??
      stringField(tool, "toolCallId") ??
      stringField(tool, "tool_call_id"),
    input: input ?? undefined,
  };
}

function nestedMeta(
  rec: Record<string, unknown>,
  key: string,
): Record<string, unknown> | null {
  const meta = asRecord(rec._meta);
  return meta ? asRecord(meta[key]) : null;
}

function mergePreview(
  preview: ToolPreview | undefined,
  path?: string,
  query?: string,
  kind?: string,
): ToolPreview | undefined {
  if (query && (!preview || preview.kind === "search" || !preview.path)) {
    return { kind: "search", ...(preview ?? {}), query };
  }
  if (!path) return preview;
  const fileName = basename(path);
  if (preview) {
    return {
      ...preview,
      path: preview.path ?? path,
      fileName: preview.fileName ?? fileName,
    };
  }
  return { kind: previewKind(kind), path, fileName };
}

function previewKind(kind?: string): ToolPreview["kind"] {
  const key = (kind ?? "").toLowerCase();
  if (key === "execute" || key === "shell") return "shell";
  if (key === "search" || key === "fetch") return "search";
  if (key === "edit" || key === "write") return "write";
  return "read";
}

function usageFromUpdate(update: Record<string, unknown>): HarnessEvent | null {
  const usage =
    asRecord(update.usage) ??
    asRecord(update.tokenUsage) ??
    asRecord(update.token_usage) ??
    (hasUsageFields(update) ? update : null);
  if (!usage) return null;
  const used =
    numberField(usage, "totalTokens") ??
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
    numberField(usage, "contextWindow") ??
    numberField(usage, "context_window") ??
    numberField(usage, "maxTokens");
  if (used == null && window == null) return null;
  return {
    type: "context",
    used: used ?? undefined,
    window: window ?? undefined,
  };
}

function hasUsageFields(rec: Record<string, unknown>): boolean {
  return (
    numberField(rec, "used") != null ||
    numberField(rec, "totalTokens") != null ||
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
  if (outputText.trim()) return cap(outputText);
  const concise = stringField(asRecord(output) ?? {}, "content_concise");
  return concise ? cap(concise) : undefined;
}

function kindFromName(name?: string): string | undefined {
  if (!name) return undefined;
  const key = name.toLowerCase().replace(/[^a-z0-9]+/g, "");
  return VARIANT_KIND[key];
}

function humanizeToolName(name: string): string {
  const cleaned = name.replace(/[_-]+/g, " ").trim();
  return cleaned ? cleaned.replace(/\b\w/g, (ch) => ch.toUpperCase()) : name;
}

function uniqueGrokModels(models: AgentModel[]): AgentModel[] {
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
    .replace(/[-_]+/g, " ")
    .replace(/\b\w/g, (ch) => ch.toUpperCase());
}

function nativeId(model: string): string {
  const trimmed = model.trim();
  if (!trimmed) return "";
  const colon = trimmed.indexOf(":");
  return colon >= 0 ? trimmed.slice(colon + 1) : trimmed;
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
    /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(text)
  );
}

function textFromContent(content: unknown, separator = ""): string {
  if (typeof content === "string") return content;
  const rec = asRecord(content);
  if (rec && typeof rec.text === "string") return rec.text;
  if (rec && rec.content != null)
    return textFromContent(rec.content, separator);
  if (Array.isArray(content)) {
    return content
      .map((item) => textFromContent(item, separator))
      .filter(Boolean)
      .join(separator);
  }
  return "";
}

function basename(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] || path;
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
  return typeof value === "number" && Number.isFinite(value)
    ? value
    : undefined;
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
