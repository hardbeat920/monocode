import type {
  AgentModel,
  ModelSetting,
  ModelSettingChoice,
} from "../../sessions/model/models";
import { MODELS } from "../../sessions/model/models";
import { CLAUDE_MODEL_CATALOG } from "../../../integrations/harness/providers/claude/claudeCatalog";
import type { HostModelCatalog, RemoteProvider } from "./protocol";

export type RemoteModelControls = {
  /** The host's current catalog entry, when it lists this model. */
  model?: AgentModel;
  settings: ModelSetting[];
  /** Why some settings are not described by the host's live catalog. */
  fallback?: "no-catalog" | "unlisted" | "saved";
};

const comparable = (id: string) =>
  id
    .replace(/^[a-z]+:/, "")
    .replace(/\[1m\]$/i, "")
    .toLowerCase()
    .replace(/^claude-/, "")
    .replace(/\./g, "-");

/** Matches a saved model to the host catalog, tolerating id scheme changes
 * such as `claude:opus-4.6` (built-in list) vs `claude:opus-4-6` (live list). */
export function findRemoteModel(
  models: readonly AgentModel[],
  id: string,
): AgentModel | undefined {
  if (!id) return undefined;
  const wanted = comparable(id);
  return (
    models.find((model) => model.id === id) ??
    models.find(
      (model) =>
        comparable(model.id) === wanted ||
        (!!model.nativeId && comparable(model.nativeId) === wanted),
    )
  );
}

const choice = (value: string, label = value): ModelSettingChoice => ({
  value,
  label,
});
const LABELS: Record<string, string> = {
  none: "None",
  minimal: "Minimal",
  low: "Low",
  medium: "Medium",
  high: "High",
  xhigh: "Extra High",
  max: "Max",
  ultracode: "Ultracode",
  ultrathink: "Ultrathink",
};
const effort = (id: string, values: string[]): ModelSetting => ({
  id,
  label: "Reasoning",
  kind: "select",
  value: "high",
  options: values.map((value) => choice(value, LABELS[value] ?? value)),
});
const toggle = (id: string, label: string): ModelSetting => ({
  id,
  label,
  kind: "toggle",
  value: "false",
  options: [choice("true", "On"), choice("false", "Off")],
});

/** Conservative definitions for settings the provider adapters understand,
 * used only when the host cannot describe the session's model. */
function knownSetting(
  provider: RemoteProvider,
  id: string,
): ModelSetting | undefined {
  if (provider === "codex") {
    if (id === "reasoningEffort")
      return effort(id, ["low", "medium", "high", "xhigh"]);
    if (id === "serviceTier")
      return {
        id,
        label: "Service Tier",
        kind: "select",
        value: "default",
        options: [choice("default", "Standard"), choice("fast", "Fast")],
      };
    return undefined;
  }
  if (provider !== "claude") return undefined;
  if (id === "effort")
    return effort(id, ["low", "medium", "high", "max", "ultrathink"]);
  if (id === "fast") return toggle(id, "Fast");
  if (id === "thinking") return toggle(id, "Thinking");
  if (id === "context")
    return {
      id,
      label: "Context",
      kind: "select",
      value: "200k",
      options: [choice("200k"), choice("1m", "1M")],
    };
  return undefined;
}

function fallbackSettings(
  provider: RemoteProvider,
  modelId: string,
  saved: Record<string, string>,
): ModelSetting[] {
  const known = findRemoteModel(
    provider === "claude"
      ? CLAUDE_MODEL_CATALOG
      : MODELS.filter((model) => model.harness === provider),
    modelId,
  )?.settings;
  const ids = [
    ...(provider === "codex"
      ? ["reasoningEffort"]
      : provider === "claude"
        ? ["effort"]
        : []),
    ...Object.keys(saved),
  ];
  const settings = [...(known ?? [])];
  for (const id of ids) {
    if (settings.some((setting) => setting.id === id)) continue;
    const setting = knownSetting(provider, id);
    if (setting) settings.push(setting);
    else if (saved[id])
      settings.push({
        id,
        label: id,
        kind: "select",
        value: saved[id],
        options: [choice(saved[id])],
      });
  }
  return settings;
}

/** Keeps a saved value selectable even when the catalog no longer offers it. */
function withSaved(
  setting: ModelSetting,
  saved: string | undefined,
): ModelSetting {
  if (!saved || setting.options.some((option) => option.value === saved))
    return setting;
  return {
    ...setting,
    options: [
      ...setting.options,
      choice(saved, `${LABELS[saved] ?? saved} (saved)`),
    ],
  };
}

/**
 * Settings controls for a remote session. Effort must never disappear just
 * because the host's catalog is loading, failed, or no longer lists the saved
 * model: those cases fall back to the session's saved values and definitions
 * the provider adapters already understand.
 */
export function remoteModelControls(
  catalog: HostModelCatalog | undefined,
  provider: RemoteProvider,
  modelId: string,
  saved: Record<string, string>,
  savedModelId?: string,
): RemoteModelControls {
  const listed = catalog?.models[provider];
  const model = listed ? findRemoteModel(listed, modelId) : undefined;
  // A newly chosen catalog model uses only what the host says it supports.
  if (model && modelId !== savedModelId)
    return { model, settings: model.settings ?? [] };
  if (!model && !savedModelId) return { settings: [] };
  const settings = [...(model?.settings ?? [])];
  let fallback: RemoteModelControls["fallback"];
  if (!model) {
    settings.push(...fallbackSettings(provider, modelId, saved));
    fallback = listed ? "unlisted" : "no-catalog";
  }
  // Saved settings the current catalog entry no longer describes stay visible.
  for (const id of Object.keys(saved)) {
    if (settings.some((setting) => setting.id === id)) continue;
    const setting = knownSetting(provider, id) ?? {
      id,
      label: id,
      kind: "select" as const,
      value: saved[id],
      options: [choice(saved[id])],
    };
    settings.push(setting);
    fallback ??= "saved";
  }
  return {
    model,
    settings: settings.map((setting) => withSaved(setting, saved[setting.id])),
    fallback,
  };
}

/** Values for `next` model: keep choices it supports, default the rest. */
export function carryModelSettings(
  settings: readonly ModelSetting[],
  current: Record<string, string>,
): Record<string, string> {
  return Object.fromEntries(
    settings.map((setting) => [
      setting.id,
      setting.options.some((option) => option.value === current[setting.id])
        ? current[setting.id]
        : setting.value,
    ]),
  );
}

export function sameModelSettings(
  a: Record<string, string> | undefined,
  b: Record<string, string> | undefined,
): boolean {
  const left = Object.entries(a ?? {});
  return (
    left.length === Object.keys(b ?? {}).length &&
    left.every(([key, value]) => b?.[key] === value)
  );
}
