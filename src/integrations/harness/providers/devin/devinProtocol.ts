import { promptBlocks, type PromptContentBlock } from "../../../../features/sessions/model/attachments";
import type { AgentModel, ModelSetting } from "../../../../features/sessions/model/models";
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

/**
 * `devin auth status` prints where `devin auth login` stored credentials:
 * `Credentials path: …`, or a `Credentials:` block whose `File:` names it.
 */
export function devinCredentialsPathFromStatus(stdout: string): string | null {
  const match =
    stdout.match(/^\s*Credentials path:\s*(.+?)\s*$/im) ??
    stdout.match(/^\s*Credentials:\s*\r?\n\s*File:\s*(.+?)\s*$/im);
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

export type DevinModelVariant = {
  value: string;
  name: string;
  effort?: string;
  fast: boolean;
};

/**
 * Devin lists every effort and speed as its own model ("Claude Opus 5.5
 * High Fast"). A family is one model whose variants differ only in those.
 */
export type DevinModelFamily = {
  key: string;
  name: string;
  variants: DevinModelVariant[];
};

const EFFORT_ORDER = ["none", "minimal", "low", "medium", "high", "xhigh", "max"];

const EFFORT_WORDS: Record<string, string> = {
  none: "none",
  minimal: "minimal",
  low: "low",
  medium: "medium",
  high: "high",
  xhigh: "xhigh",
  "x-high": "xhigh",
  "extra high": "xhigh",
  max: "max",
};

const EFFORT_LABELS: Record<string, string> = {
  none: "No thinking",
  minimal: "Minimal",
  low: "Low",
  medium: "Medium",
  high: "High",
  xhigh: "Extra High",
  max: "Max",
};

/** The `model` select's choices, from a session result or its config options. */
export function devinModelChoices(
  raw: unknown,
): Array<{ value: string; label: string }> {
  const options = Array.isArray(raw) ? raw : asRecord(raw)?.configOptions;
  const option = (Array.isArray(options) ? options : [])
    .map(asRecord)
    .find((item) => item?.id === "model" || item?.category === "model");
  return flattenChoices(option?.options);
}

/** Names are more regular than ids: `swe-1-7-lightning` is "Lightning Max". */
function parseDevinVariant(name: string): { base: string; effort?: string; fast: boolean } {
  let rest = name.trim();
  let fast = false;
  if (/\s+fast$/i.test(rest)) {
    fast = true;
    rest = rest.replace(/\s+fast$/i, "");
  }
  // A context size trails the effort ("GLM-5.2 High 1M") and names its own model.
  const context = /\s+\d+(?:\.\d+)?[KM]$/i.exec(rest)?.[0] ?? "";
  rest = rest.slice(0, rest.length - context.length);
  if (/\s+no thinking$/i.test(rest)) {
    return { base: rest.replace(/\s+no thinking$/i, "") + context, effort: "none", fast };
  }
  // "Medium Thinking" is an effort; a bare "Thinking" suffix is part of the name.
  const match =
    /^(.*\S)\s+(none|minimal|low|medium|high|xhigh|x-high|extra high|max)(\s+thinking)?$/i.exec(rest);
  if (match) {
    return { base: match[1] + context, effort: EFFORT_WORDS[match[2].toLowerCase()], fast };
  }
  return { base: rest + context, fast };
}

function familyKey(base: string): string {
  return base.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "");
}

export function devinModelFamilies(
  choices: Array<{ value: string; label: string }>,
): DevinModelFamily[] {
  const families = new Map<string, DevinModelFamily>();
  const seen = new Set<string>();
  for (const { value, label } of choices) {
    if (seen.has(value)) continue;
    seen.add(value);
    const parsed = parseDevinVariant(label);
    const key = familyKey(parsed.base) || value;
    const family = families.get(key) ?? { key, name: parsed.base, variants: [] };
    family.variants.push({ value, name: label, effort: parsed.effort, fast: parsed.fast });
    families.set(key, family);
  }
  return [...families.values()].flatMap((family) =>
    isCleanFamily(family)
      ? [family]
      : // Ambiguous names stay as separate, exactly named models.
        family.variants.map((variant) => ({
          key: variant.value,
          name: variant.name,
          variants: [{ ...variant, effort: undefined, fast: false }],
        })),
  );
}

/** Every variant must differ by effort/speed, and effort must be all-or-none. */
function isCleanFamily(family: DevinModelFamily): boolean {
  if (family.variants.length < 2) return true;
  const withEffort = family.variants.filter((variant) => variant.effort).length;
  if (withEffort !== 0 && withEffort !== family.variants.length) return false;
  const pairs = new Set(family.variants.map((variant) => `${variant.effort}|${variant.fast}`));
  return pairs.size === family.variants.length;
}

/** Devin lists a model's default variant first; Fast is opt-in. */
function defaultVariant(family: DevinModelFamily): DevinModelVariant {
  return family.variants.find((variant) => !variant.fast) ?? family.variants[0];
}

function familyModel(family: DevinModelFamily): AgentModel {
  if (family.variants.length === 1) {
    const [only] = family.variants;
    return { id: `devin:${only.value}`, harness: "devin", name: only.name, nativeId: only.value };
  }
  const fallback = defaultVariant(family);
  const efforts = EFFORT_ORDER.filter((effort) =>
    family.variants.some((variant) => variant.effort === effort),
  );
  const settings: ModelSetting[] = [];
  if (efforts.length > 1) {
    settings.push({
      id: "effort",
      label: "Effort",
      kind: "select",
      value: fallback.effort ?? efforts[0],
      options: efforts.map((value) => ({ value, label: EFFORT_LABELS[value] ?? value })),
    });
  }
  if (family.variants.some((variant) => variant.fast)) {
    settings.push({
      id: "fast",
      label: "Fast",
      kind: "toggle",
      value: "false",
      options: [
        { value: "false", label: "Off" },
        { value: "true", label: "Fast" },
      ],
    });
  }
  return {
    id: `devin:${family.key}`,
    harness: "devin",
    name: family.name,
    nativeId: family.key,
    settings: settings.length > 0 ? settings : undefined,
  };
}

/**
 * The exact Devin model value for a family and the picker's settings.
 * Ids that are not a family (single models, older saved ids) pass through.
 */
export function devinModelValue(
  families: DevinModelFamily[],
  nativeId: string,
  settings: Record<string, string> = {},
): string {
  const family = families.find((item) => item.key === nativeId);
  if (!family || family.variants.length < 2) return nativeId;
  const fallback = defaultVariant(family);
  const effort = settings.effort ?? fallback.effort;
  const fast = settings.fast != null ? settings.fast === "true" : fallback.fast;
  return (
    family.variants.find((variant) => variant.effort === effort && variant.fast === fast) ??
    family.variants.find((variant) => variant.effort === effort && !variant.fast) ??
    fallback
  ).value;
}

/** Models come from the `model` select in Devin's session config options. */
export function modelsFromDevinSession(raw: unknown): AgentModel[] {
  const rec = asRecord(raw);
  const options = Array.isArray(rec?.configOptions) ? rec.configOptions : [];
  const option = options
    .map(asRecord)
    .find((item) => item?.id === "model" || item?.category === "model");
  const current =
    typeof option?.currentValue === "string" ? option.currentValue : "";
  const families = devinModelFamilies(devinModelChoices(raw));
  // setHarnessModels defaults to the first entry, so lead with the model the
  // account is already configured to use.
  const index = families.findIndex((family) =>
    family.variants.some((variant) => variant.value === current),
  );
  if (index > 0) families.unshift(...families.splice(index, 1));
  return families.map(familyModel);
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
