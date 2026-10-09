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
 * coding mode still accepts edits and prompts for commands. Supervised adds a
 * permission rule instead (see `devinAsksBeforeEdits`). Plan intent uses
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
 * Supervised children start with an `ask` rule for every write, so workspace
 * edits reach MonoCode's approval prompt instead of being accepted silently.
 * The rule is process-wide: changing it restarts the child.
 */
export function devinAsksBeforeEdits(runtimeMode: RuntimeMode): boolean {
  return runtimeMode === "supervised";
}

export const DEVIN_ASK_EDITS_RULE = "Write(**)";

/**
 * The user's Devin config with the edit rule added; `--config` replaces the
 * user config, so their own settings must come along. Throws for anything it
 * cannot extend: Supervised must not start without the rule. Mirrors
 * `devin_config.rs` for the headless host.
 */
export function devinAskEditsConfig(user: string | null): string {
  const help = "Supervised mode needs it to make Devin ask before edits.";
  let config: unknown = {};
  if (user?.trim()) {
    try {
      config = JSON.parse(user);
    } catch (error) {
      throw new Error(`Devin's config.json is not valid JSON (${errorDetail(error)}). ${help}`);
    }
  }
  const root = asRecord(config);
  if (!root || Array.isArray(config)) {
    throw new Error(`Devin's config.json is not a JSON object. ${help}`);
  }
  const permissions = root.permissions ?? {};
  if (!asRecord(permissions) || Array.isArray(permissions)) {
    throw new Error(`Devin's config.json \`permissions\` is not an object. ${help}`);
  }
  const ask = (permissions as Record<string, unknown>).ask ?? [];
  if (!Array.isArray(ask)) {
    throw new Error(`Devin's config.json \`permissions.ask\` is not a list. ${help}`);
  }
  return JSON.stringify(
    {
      ...root,
      permissions: {
        ...(permissions as Record<string, unknown>),
        ask: ask.includes(DEVIN_ASK_EDITS_RULE) ? ask : [...ask, DEVIN_ASK_EDITS_RULE],
      },
    },
    null,
    2,
  );
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

/**
 * Default credential locations when `devin auth status` is unavailable, in
 * the order the usage fetch in `devin_usage.rs` reads them.
 */
export function devinCredentialsCandidates(home: string): string[] {
  const base = home.replace(/[\\/]+$/, "");
  return [
    `${base}/AppData/Roaming/devin/credentials.toml`,
    `${base}/.local/share/devin/credentials.toml`,
    `${base}/.config/devin/credentials.toml`,
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
  const detail = errorDetail(error);
  const auth = withAuthHelp(detail);
  if (auth) return new Error(auth);
  if (/timed out/i.test(detail)) {
    return new Error(`Devin did not start. ${DEVIN_AUTH_HELP}`);
  }
  return new Error(`Devin did not start. ${detail}`);
}

/**
 * A failure inside a running session. Unlike startup, a timeout here means
 * the turn stalled, not that the login is missing.
 */
export function devinTurnError(error: unknown): Error {
  const detail = errorDetail(error);
  const auth = withAuthHelp(detail);
  if (auth) return new Error(auth);
  if (/timed out/i.test(detail)) {
    return new Error("Devin stopped responding before the turn finished.");
  }
  return new Error(detail);
}

/**
 * `session/load` failed because Devin no longer has the conversation, the one
 * case where replacing it with a new session loses nothing.
 */
export function devinSessionMissing(error: unknown): boolean {
  return /session.*not found|not found.*session|no such session|unknown session/i.test(
    errorDetail(error),
  );
}

/** A restore that failed for any other reason; the binding is kept for a retry. */
export function devinRestoreError(error: unknown): Error {
  const detail = errorDetail(error);
  const auth = withAuthHelp(detail);
  if (auth) return new Error(auth);
  if (/timed out/i.test(detail)) {
    return new Error(
      "Devin timed out restoring the previous conversation. Send again to retry.",
    );
  }
  return new Error(`Devin could not restore the previous conversation. ${detail}`);
}

function errorDetail(error: unknown): string {
  return (error instanceof Error ? error.message : String(error)).trim();
}

function withAuthHelp(detail: string): string | null {
  if (detail.includes(DEVIN_AUTH_HELP)) return detail;
  if (/auth|credential|api key|log ?in|sign.in/i.test(detail)) {
    return `${detail}\n\n${DEVIN_AUTH_HELP}`;
  }
  return null;
}

export type DevinModelVariant = {
  value: string;
  name: string;
  effort?: string;
  fast: boolean;
  /** `fusion-*` ids pair a lead model and effort with a sidekick model. */
  fusion?: {
    lead: string;
    leadName: string;
    leadEffort?: string;
    sidekick: string;
    sidekickName: string;
  };
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

export type DevinThoughtLevel = {
  /** Config option id, usually `thought_level`. */
  id: string;
  choices: Array<{ value: string; label: string }>;
  current?: string;
};

/**
 * Newer Devin CLIs moved effort off the model list into a session-wide
 * `thought_level` select ("No Thinking" / "High" / "Max").
 */
export function devinThoughtLevel(raw: unknown): DevinThoughtLevel | null {
  const options = Array.isArray(raw) ? raw : asRecord(raw)?.configOptions;
  const option = (Array.isArray(options) ? options : [])
    .map(asRecord)
    .find(
      (item) =>
        item?.category === "thought_level" || item?.id === "thought_level",
    );
  const choices = flattenChoices(option?.options);
  if (!option || choices.length === 0) return null;
  const current = stringField(option, "currentValue");
  return { id: String(option.id), choices, ...(current ? { current } : {}) };
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

/** Effort survives as an id suffix on CLIs that dropped it from the names. */
const EFFORT_ID_SUFFIX = /-(none|minimal|low|medium|high|xhigh|max)$/i;

/** `fusion-<lead>-<effort>-sidekick-<sidekick>` → lead/sidekick parts. */
function devinFusionParts(
  value: string,
  label: string,
): DevinModelVariant["fusion"] {
  if (!value.startsWith("fusion-")) return undefined;
  const rest = value.slice("fusion-".length);
  const at = rest.indexOf("-sidekick-");
  if (at <= 0) return undefined;
  const leadPart = rest.slice(0, at);
  const sidekick = rest.slice(at + "-sidekick-".length);
  if (!leadPart || !sidekick) return undefined;
  const leadEffort = EFFORT_ID_SUFFIX.exec(leadPart)?.[1]?.toLowerCase();
  const lead = leadEffort
    ? leadPart.slice(0, leadPart.length - leadEffort.length - 1)
    : leadPart;
  // "Fusion (Claude Fable 5.1 Medium + SWE-2 Medium)" names both halves.
  let leadName = lead;
  let sidekickName = sidekick;
  const inner = /^fusion\s*\((.*)\)\s*$/i.exec(label.trim())?.[1];
  const plus = inner?.indexOf(" + ") ?? -1;
  if (inner && plus > 0) {
    leadName = parseDevinVariant(inner.slice(0, plus).trim()).base;
    sidekickName = inner.slice(plus + 3).trim();
  }
  return { lead, leadName, leadEffort, sidekick, sidekickName };
}

export function devinModelFamilies(
  choices: Array<{ value: string; label: string }>,
): DevinModelFamily[] {
  const families = new Map<string, DevinModelFamily>();
  const seen = new Set<string>();
  for (const { value, label } of choices) {
    if (seen.has(value)) continue;
    seen.add(value);
    const fusion = devinFusionParts(value, label);
    const parsed = fusion ? { base: "Fusion", fast: false } : parseDevinVariant(label);
    const effort = fusion
      ? undefined
      : (parsed.effort ?? EFFORT_ID_SUFFIX.exec(value)?.[1]?.toLowerCase());
    const key = fusion ? "fusion" : familyKey(parsed.base) || value;
    const family = families.get(key) ?? {
      key,
      name: fusion ? "Fusion" : parsed.base,
      variants: [],
    };
    family.variants.push({
      value,
      name: label,
      effort,
      fast: parsed.fast,
      ...(fusion ? { fusion } : {}),
    });
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
  // Fusion variants differ by lead and sidekick, not effort or speed.
  if (family.variants.some((variant) => variant.fusion)) return true;
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

/** The session-wide effort select newer CLIs expose as `thought_level`. */
function thoughtLevelSetting(thought: DevinThoughtLevel): ModelSetting {
  return {
    id: "effort",
    label: "Effort",
    kind: "select",
    value: thought.current ?? thought.choices[0].value,
    options: thought.choices,
  };
}

/**
 * A single model's picker name. With an Effort select the effort is shown
 * there, so a default baked into Devin's label ("GLM-5.2 High") is dropped;
 * without one the id's fixed effort is named the way Devin's picker does.
 */
function singleModelName(
  family: DevinModelFamily,
  variant: DevinModelVariant,
  thought: DevinThoughtLevel | null,
): string {
  const parsed = parseDevinVariant(variant.name);
  if (thought) {
    // Split ambiguous families keep Devin's exact label.
    return parsed.effort && familyKey(parsed.base) === family.key
      ? `${parsed.base}${parsed.fast ? " Fast" : ""}`
      : variant.name;
  }
  return !parsed.effort && variant.effort && variant.effort !== "none"
    ? `${variant.name} ${EFFORT_LABELS[variant.effort] ?? variant.effort}`
    : variant.name;
}

function familyModel(
  family: DevinModelFamily,
  thoughtFor: (value: string) => DevinThoughtLevel | null,
): AgentModel {
  if (family.variants.length > 1 && family.variants[0]?.fusion) {
    return fusionModel(family);
  }
  if (family.variants.length === 1) {
    const [only] = family.variants;
    const thought = only.fusion ? null : thoughtFor(only.value);
    return {
      id: `devin:${only.value}`,
      harness: "devin",
      name: singleModelName(family, only, thought),
      nativeId: only.value,
      ...(thought ? { settings: [thoughtLevelSetting(thought)] } : {}),
    };
  }
  const fallback = defaultVariant(family);
  const thought = thoughtFor(fallback.value);
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
  } else if (thought) {
    settings.push(thoughtLevelSetting(thought));
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

/** Devin's own picker shows one Fusion row with lead/effort/sidekick selects. */
function fusionModel(family: DevinModelFamily): AgentModel {
  const variants = family.variants.filter((variant) => variant.fusion);
  const first = variants[0].fusion!;
  const leads = new Map<string, string>();
  const sidekicks = new Map<string, string>();
  for (const variant of variants) {
    const fusion = variant.fusion!;
    if (!leads.has(fusion.lead)) leads.set(fusion.lead, fusion.leadName);
    if (!sidekicks.has(fusion.sidekick)) {
      sidekicks.set(fusion.sidekick, fusion.sidekickName);
    }
  }
  const efforts = EFFORT_ORDER.filter((effort) =>
    variants.some((variant) => variant.fusion!.leadEffort === effort),
  );
  // Devin pairs most leads at a single effort ("Claude Opus 5 High"); a
  // select is only meaningful when some lead really offers a choice.
  const leadEfforts = new Map<string, Set<string>>();
  for (const variant of variants) {
    const { lead, leadEffort } = variant.fusion!;
    if (!leadEffort) continue;
    leadEfforts.set(lead, (leadEfforts.get(lead) ?? new Set()).add(leadEffort));
  }
  const effortChoice = [...leadEfforts.values()].some((set) => set.size > 1);
  const settings: ModelSetting[] = [
    {
      id: "lead",
      label: "Lead",
      kind: "select",
      value: first.lead,
      options: [...leads].map(([value, label]) => ({ value, label })),
    },
  ];
  if (effortChoice && efforts.length > 1) {
    settings.push({
      id: "effort",
      label: "Effort",
      kind: "select",
      value: first.leadEffort ?? efforts[0],
      options: efforts.map((value) => ({
        value,
        label: EFFORT_LABELS[value] ?? value,
      })),
    });
  }
  settings.push({
    id: "sidekick",
    label: "Sidekick",
    kind: "select",
    value: first.sidekick,
    options: [...sidekicks].map(([value, label]) => ({ value, label })),
  });
  return {
    id: "devin:fusion",
    harness: "devin",
    name: "Fusion",
    nativeId: "fusion",
    settings,
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
  if (!family) return nativeId;
  if (family.variants.some((variant) => variant.fusion)) {
    return fusionModelValue(family, settings);
  }
  // A family key saved by an older catalog ("swe-2") names a model Devin now
  // lists under its full id ("swe-2-high").
  if (family.variants.length < 2) return family.variants[0]?.value ?? nativeId;
  const fallback = defaultVariant(family);
  const effort = settings.effort ?? fallback.effort;
  const fast = settings.fast != null ? settings.fast === "true" : fallback.fast;
  return (
    family.variants.find((variant) => variant.effort === effort && variant.fast === fast) ??
    family.variants.find((variant) => variant.effort === effort && !variant.fast) ??
    fallback
  ).value;
}

/** Pick the fusion id for the lead/effort/sidekick settings, falling back sanely. */
function fusionModelValue(
  family: DevinModelFamily,
  settings: Record<string, string>,
): string {
  const variants = family.variants;
  const first = variants.find((variant) => variant.fusion)!;
  const lead = settings.lead ?? first.fusion!.lead;
  const forLead = variants.filter((variant) => variant.fusion!.lead === lead);
  const pool = forLead.length > 0 ? forLead : variants;
  const effort = settings.effort;
  const matching = effort
    ? pool.filter((variant) => variant.fusion!.leadEffort === effort)
    : [];
  // A lead may offer only one effort; fall back to what it actually has.
  const withEffort = matching.length > 0 ? matching : pool;
  const sidekick = settings.sidekick ?? first.fusion!.sidekick;
  return (
    withEffort.find((variant) => variant.fusion!.sidekick === sidekick) ??
    withEffort[0]
  ).value;
}

/**
 * Each model's own `thought_level` choices, keyed by model value: Devin's
 * efforts differ per model (SWE-2: medium/high/max, Claude Opus 4.6: none).
 * Null when the model has no effort select.
 */
export type DevinThoughtLevels = ReadonlyMap<string, DevinThoughtLevel | null>;

/**
 * The offered effort closest to `value`, preferring the lower level on a tie,
 * so a saved effort still means something on a model with other levels.
 */
export function devinNearestEffort(
  value: string,
  choices: readonly string[],
): string | null {
  if (choices.includes(value)) return value;
  const rank = EFFORT_ORDER.indexOf(value);
  if (rank < 0) return null;
  let best: string | null = null;
  let bestDistance = Infinity;
  // Ascending, so the first of two equally close levels is the lower one.
  for (const choice of EFFORT_ORDER.filter((effort) => choices.includes(effort))) {
    const distance = Math.abs(EFFORT_ORDER.indexOf(choice) - rank);
    if (distance >= bestDistance) continue;
    best = choice;
    bestDistance = distance;
  }
  return best;
}

/**
 * Models come from the `model` select in Devin's session config options.
 * `levels` holds per-model efforts; without it (or for a model it misses)
 * the session's own `thought_level` stands in.
 */
export function modelsFromDevinSession(
  raw: unknown,
  levels?: DevinThoughtLevels,
): AgentModel[] {
  const rec = asRecord(raw);
  const options = Array.isArray(rec?.configOptions) ? rec.configOptions : [];
  const option = options
    .map(asRecord)
    .find((item) => item?.id === "model" || item?.category === "model");
  const current =
    typeof option?.currentValue === "string" ? option.currentValue : "";
  const families = devinModelFamilies(devinModelChoices(raw));
  const thought = devinThoughtLevel(raw);
  // setHarnessModels defaults to the first entry, so lead with the model the
  // account is already configured to use.
  const index = families.findIndex((family) =>
    family.variants.some((variant) => variant.value === current),
  );
  if (index > 0) families.unshift(...families.splice(index, 1));
  const thoughtFor = (value: string) =>
    levels?.has(value) ? (levels.get(value) ?? null) : thought;
  return families.map((family) => familyModel(family, thoughtFor));
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
