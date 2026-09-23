import { promptBlocks, type PromptContentBlock } from "../../../../features/sessions/model/attachments";
import type { AgentModel, ModelSetting } from "../../../../features/sessions/model/models";
import type { Attachment, RuntimeMode } from "../../../../features/sessions/model/session";

export const DROID_AUTH_HELP =
  "Sign in by running `droid` once in Terminal and using /login, or set FACTORY_API_KEY, then retry.";

/** `droid exec --output-format acp` speaks standard ACP over stdio. */
export const DROID_ACP_ARGS = ["exec", "--output-format", "acp"];

export type DroidModeId =
  "normal" | "spec" | "auto-low" | "auto-medium" | "auto-high";

export type DroidConfigOption = {
  id: string;
  category?: string;
  currentValue?: string;
  options: { value: string; name: string }[];
};

/**
 * Droid's autonomy levels. Anything the level does not auto-approve comes
 * back as `session/request_permission`, so MonoCode still gets the final say.
 */
export function droidModeId(
  runtimeMode: RuntimeMode,
  planning = false,
): DroidModeId {
  if (planning) return "spec";
  if (runtimeMode === "supervised") return "normal";
  if (runtimeMode === "auto-accept-edits") return "auto-low";
  if (runtimeMode === "auto") return "auto-medium";
  return "auto-high";
}

/** Droid accepts the standard ACP text, image, and resource-link blocks. */
export function droidPromptBlocks(
  text: string,
  attachments: Attachment[] = [],
): PromptContentBlock[] {
  return promptBlocks(text, attachments);
}

export function droidSessionId(result: unknown): string | undefined {
  const rec = asRecord(result);
  const id = rec?.sessionId ?? rec?.session_id ?? rec?.id;
  return typeof id === "string" && id.trim() ? id.trim() : undefined;
}

export function readDroidConfigOptions(raw: unknown): DroidConfigOption[] {
  if (!Array.isArray(raw)) return [];
  return raw.flatMap((item) => {
    const rec = asRecord(item);
    const id = typeof rec?.id === "string" ? rec.id : "";
    if (!rec || !id) return [];
    const options = Array.isArray(rec.options)
      ? rec.options.flatMap((entry) => {
          const option = asRecord(entry);
          const value = option?.value;
          if (typeof value !== "string" || !value) return [];
          const name = typeof option?.name === "string" ? option.name : value;
          return [{ value, name }];
        })
      : [];
    return [
      {
        id,
        category: typeof rec.category === "string" ? rec.category : undefined,
        currentValue:
          typeof rec.currentValue === "string" ? rec.currentValue : undefined,
        options,
      },
    ];
  });
}

/** Config options from a session/new result or a `config_option_update`. */
export function droidConfigOptionsFrom(value: unknown): DroidConfigOption[] | null {
  const rec = asRecord(value);
  const update = asRecord(rec?.update) ?? rec;
  const raw = update?.configOptions ?? update?.config_options;
  return Array.isArray(raw) ? readDroidConfigOptions(raw) : null;
}

export function droidModelConfig(
  options: DroidConfigOption[],
): DroidConfigOption | undefined {
  return options.find(
    (option) => option.id === "model" || option.category === "model",
  );
}

export function droidEffortConfig(
  options: DroidConfigOption[],
): DroidConfigOption | undefined {
  return options.find(
    (option) =>
      option.id === "reasoning_effort" || option.category === "thought_level",
  );
}

export function droidCurrentModelId(result: unknown): string | undefined {
  const rec = asRecord(result);
  const models = asRecord(rec?.models);
  const value =
    models?.currentModelId ??
    models?.current_model_id ??
    droidModelConfig(droidConfigOptionsFrom(result) ?? [])?.currentValue;
  return typeof value === "string" && value.trim() ? value.trim() : undefined;
}

/**
 * Droid's reasoning levels differ per model (`off`/`none`, up to `max`).
 * Expose them as MonoCode's standard `effort` select; one choice is no choice.
 */
export function droidEffortSetting(
  config: DroidConfigOption | undefined,
): ModelSetting | undefined {
  if (!config || config.options.length < 2) return undefined;
  const value =
    config.currentValue &&
    config.options.some((option) => option.value === config.currentValue)
      ? config.currentValue
      : config.options[0].value;
  return {
    id: "effort",
    label: "Reasoning",
    kind: "select",
    value,
    options: config.options.map((option) => ({
      value: option.value,
      label: option.name,
    })),
  };
}

/** MonoCode's effort value mapped onto what this Droid model accepts. */
export function droidEffortValue(
  config: DroidConfigOption | undefined,
  settings: Record<string, string> | undefined,
): string | undefined {
  const wanted = (settings?.effort ?? settings?.reasoning)?.trim();
  if (!config || !wanted) return undefined;
  const aliases: Record<string, string[]> = {
    xhigh: ["xhigh", "extra-high"],
    "extra-high": ["xhigh", "extra-high"],
    off: ["off", "none"],
    none: ["none", "off"],
  };
  for (const candidate of aliases[wanted] ?? [wanted]) {
    if (config.options.some((option) => option.value === candidate)) {
      return candidate;
    }
  }
  return undefined;
}

/** Read Droid's ACP SessionModelState into MonoCode catalog entries. */
export function modelsFromDroidSession(
  result: unknown,
  effortByModel: ReadonlyMap<string, DroidConfigOption> = new Map(),
): AgentModel[] {
  const rec = asRecord(result);
  const state = asRecord(rec?.models);
  let raw: unknown = state?.availableModels ?? state?.available_models;
  if (!Array.isArray(raw)) {
    raw = droidModelConfig(droidConfigOptionsFrom(result) ?? [])?.options.map(
      (option) => ({ modelId: option.value, name: option.name }),
    );
  }
  if (!Array.isArray(raw)) return [];

  const seen = new Set<string>();
  const models: AgentModel[] = [];
  for (const item of raw) {
    const model = asRecord(item);
    if (!model) continue;
    const nativeId = String(
      model.modelId ?? model.model_id ?? model.value ?? model.id ?? "",
    ).trim();
    if (!nativeId || seen.has(nativeId)) continue;
    seen.add(nativeId);
    const name = String(model.name ?? model.title ?? nativeId).trim();
    const effort = droidEffortSetting(effortByModel.get(nativeId));
    models.push({
      id: `droid:${nativeId}`,
      harness: "droid",
      name: name || nativeId,
      nativeId,
      ...(effort ? { settings: [effort] } : {}),
    });
  }

  // Droid's configured default is the right first pick for a new session.
  const current = droidCurrentModelId(result);
  if (current) {
    const index = models.findIndex((model) => model.nativeId === current);
    if (index > 0) models.unshift(...models.splice(index, 1));
  }
  return models;
}

/**
 * Droid reports failures as a generic `Internal error` with the useful text
 * in `data`, often an HTTP status followed by a JSON body with `detail`.
 */
export function droidErrorMessage(error: unknown): string {
  const message = error instanceof Error ? error.message : String(error);
  const data = asRecord(error)?.data;
  if (typeof data !== "string" || !data.trim()) return message;
  const brace = data.indexOf("{");
  if (brace >= 0) {
    try {
      const body = asRecord(JSON.parse(data.slice(brace)));
      const detail = body?.detail ?? body?.message ?? body?.error;
      if (typeof detail === "string" && detail.trim()) return detail.trim();
    } catch {
      // Not JSON; fall through to the raw data.
    }
  }
  return data.trim();
}

/**
 * Droid also streams the same failure as an `Error: <status> {...}` message
 * chunk. The prompt rejection already reports it as a session error.
 */
export function isDroidErrorEcho(params: unknown): boolean {
  const update = asRecord(asRecord(params)?.update);
  if (update?.sessionUpdate !== "agent_message_chunk") return false;
  const text = asRecord(update.content)?.text;
  return typeof text === "string" && /^Error: \d{3} \{/.test(text);
}

export function isDroidAuthError(detail: string): boolean {
  return /(?:auth(?:entication)? required|not (?:logged in|authenticated)|unauthori[sz]ed|invalid api key|FACTORY_API_KEY|please (?:log|sign) in)/i.test(
    detail,
  );
}

export function droidStartupError(error: unknown): Error {
  const detail = droidErrorMessage(error);
  if (isDroidAuthError(detail)) {
    return new Error(`${detail.trim()}\n\n${DROID_AUTH_HELP}`);
  }
  if (/timed out/i.test(detail)) {
    return new Error(`Factory Droid did not start. ${DROID_AUTH_HELP}`);
  }
  return new Error(`Factory Droid did not start. ${detail}`);
}

/** Plan text Droid attaches when leaving spec mode (ExitSpecMode). */
export function droidSpecPlan(params: unknown): string | undefined {
  const rec = asRecord(params);
  const tool = asRecord(rec?.toolCall) ?? asRecord(rec?.tool_call);
  if (!tool) return undefined;
  const raw = asRecord(tool.rawInput) ?? asRecord(tool.raw_input);
  const title = String(tool.title ?? "");
  const isSpec =
    tool.kind === "switch_mode" || /spec/i.test(title) || raw?.plan != null;
  if (!isSpec) return undefined;
  for (const key of ["plan", "spec", "content", "markdown"]) {
    const value = raw?.[key];
    if (typeof value === "string" && value.trim()) return value.trim();
  }
  const content = Array.isArray(tool.content) ? tool.content : [];
  const text = content
    .map((item) => {
      const block = asRecord(asRecord(item)?.content) ?? asRecord(item);
      return typeof block?.text === "string" ? block.text : "";
    })
    .filter(Boolean)
    .join("\n")
    .trim();
  return text || undefined;
}

export function asRecord(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}
