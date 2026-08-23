import type { HarnessId } from "./session";
import { HARNESSES } from "./session";

export type ModelSettingChoice = {
  value: string;
  label: string;
};

export type ModelSetting = {
  id: string;
  label: string;
  kind: "select" | "toggle";
  value: string;
  options: ModelSettingChoice[];
  description?: string;
};

export type AgentModel = {
  id: string;
  harness: HarnessId;
  name: string;
  nativeId?: string;
  settings?: ModelSetting[];
  /** Context window, when the harness catalog reports one. */
  contextWindow?: number;
};

export const MODELS: AgentModel[] = [
  {
    id: "claude:sonnet-5",
    harness: "claude",
    name: "Claude Sonnet 5",
    nativeId: "claude-sonnet-5",
  },
  {
    id: "claude:opus-5",
    harness: "claude",
    name: "Claude Opus 5",
    nativeId: "claude-opus-5",
  },
  {
    id: "claude:fable-5",
    harness: "claude",
    name: "Claude Fable 5",
    nativeId: "claude-fable-5",
  },
  {
    id: "claude:opus-4.6",
    harness: "claude",
    name: "Opus 4.6",
    nativeId: "claude-opus-4-6",
  },
  {
    id: "claude:sonnet-4.6",
    harness: "claude",
    name: "Sonnet 4.6",
    nativeId: "claude-sonnet-4-6",
  },
  {
    id: "claude:haiku-4.5",
    harness: "claude",
    name: "Haiku 4.5",
    nativeId: "claude-haiku-4-5",
  },
  {
    id: "claude:opus-4.5",
    harness: "claude",
    name: "Opus 4.5",
    nativeId: "claude-opus-4-5",
  },

  {
    id: "cursor:composer-2.5",
    harness: "cursor",
    name: "Composer 2.5",
    nativeId: "composer-2.5",
  },
  {
    id: "cursor:gpt-5.4",
    harness: "cursor",
    name: "GPT-5.4",
    nativeId: "gpt-5.4",
  },
  {
    id: "cursor:claude-sonnet-4-6",
    harness: "cursor",
    name: "Sonnet 4.6",
    nativeId: "claude-sonnet-4-6",
  },
  {
    id: "cursor:grok-4.6",
    harness: "cursor",
    name: "Cursor Grok 4.6",
    nativeId: "grok-4.6",
  },

  { id: "opencode:glm-5", harness: "opencode", name: "GLM 5" },
  { id: "opencode:minimax-m2.5", harness: "opencode", name: "MiniMax M2.5" },
  { id: "opencode:kimi-k2.5", harness: "opencode", name: "Kimi K2.5" },
  {
    id: "opencode:deepseek-v4-flash",
    harness: "opencode",
    name: "DeepSeek V4 Flash",
  },
  { id: "opencode:qwen-3.5", harness: "opencode", name: "Qwen 3.5" },
  { id: "opencode:grok-4.5", harness: "opencode", name: "Grok 4.5" },
  {
    id: "opencode:claude-sonnet-4.6",
    harness: "opencode",
    name: "Claude Sonnet 4.6",
  },
  { id: "opencode:gpt-5.4", harness: "opencode", name: "GPT-5.4" },
  {
    id: "pi:default",
    harness: "pi",
    name: "Default",
    nativeId: "",
  },
  {
    id: "fx:zai/glm-5.2-fast",
    harness: "fx",
    name: "GLM 5.2 Fast",
    nativeId: "zai/glm-5.2-fast",
  },

  {
    id: "amp:low",
    harness: "amp",
    name: "Low",
    nativeId: "low",
    settings: [
      {
        id: "fast",
        label: "Fast",
        kind: "toggle",
        value: "false",
        options: [
          { value: "true", label: "On" },
          { value: "false", label: "Off" },
        ],
      },
    ],
  },
  {
    id: "amp:medium",
    harness: "amp",
    name: "Medium",
    nativeId: "medium",
    settings: [
      {
        id: "fast",
        label: "Fast",
        kind: "toggle",
        value: "false",
        options: [
          { value: "true", label: "On" },
          { value: "false", label: "Off" },
        ],
      },
    ],
  },
  {
    id: "amp:high",
    harness: "amp",
    name: "High",
    nativeId: "high",
    settings: [
      {
        id: "fast",
        label: "Fast",
        kind: "toggle",
        value: "false",
        options: [
          { value: "true", label: "On" },
          { value: "false", label: "Off" },
        ],
      },
    ],
  },
  {
    id: "amp:ultra",
    harness: "amp",
    name: "Ultra",
    nativeId: "ultra",
    settings: [
      {
        id: "fast",
        label: "Fast",
        kind: "toggle",
        value: "false",
        options: [
          { value: "true", label: "On" },
          { value: "false", label: "Off" },
        ],
      },
    ],
  },
];

export const DEFAULT_MODEL_ID: Record<HarnessId, string> = {
  claude: "claude:sonnet-5",
  codex: "",
  cursor: "cursor:composer-2.5",
  opencode: "opencode:glm-5",
  pi: "pi:default",
  fx: "fx:zai/glm-5.2-fast",
  amp: "amp:medium",
};

const FAVORITES_KEY = "monocode.favoriteModels";
const MODEL_PICKER_TAB_KEY = "monocode.modelPickerTab";
const LAST_MODEL_KEY = "monocode.lastModel";
const LAST_MODEL_SETTINGS_KEY = "monocode.lastModelSettings";

export type ModelPickerTab = "favorites" | HarnessId;

export type LastModelChoice = {
  harness: HarnessId;
  model: string;
};

const HARNESS_ORDER: HarnessId[] = [
  "claude",
  "codex",
  "cursor",
  "opencode",
  "pi",
  "fx",
  "amp",
];

let overlays: Partial<Record<HarnessId, AgentModel[]>> = {};
let overlayDefaults: Partial<Record<HarnessId, string>> = {};
let catalogVersion = 0;
const listeners = new Set<() => void>();

function emit() {
  catalogVersion += 1;
  for (const listener of listeners) listener();
}

export function subscribeModels(onStoreChange: () => void): () => void {
  listeners.add(onStoreChange);
  return () => {
    listeners.delete(onStoreChange);
  };
}

export function getModelSnapshot(): number {
  return catalogVersion;
}

export function setHarnessModels(harness: HarnessId, models: AgentModel[]) {
  if (models.length === 0) return;
  overlays = { ...overlays, [harness]: models };
  overlayDefaults = {
    ...overlayDefaults,
    [harness]: pickDefaultId(harness, models),
  };
  emit();
}

export function defaultModelId(harness: HarnessId): string {
  return overlayDefaults[harness] ?? DEFAULT_MODEL_ID[harness];
}

export function modelsFor(harness: HarnessId): AgentModel[] {
  return overlays[harness] ?? MODELS.filter((model) => model.harness === harness);
}

export function allModels(): AgentModel[] {
  return HARNESS_ORDER.flatMap(modelsFor);
}

export function findModel(id: string): AgentModel | undefined {
  return allModels().find((model) => model.id === id);
}

export function resolveModel(harness: HarnessId, id?: string): AgentModel {
  const available = modelsFor(harness);
  if (id) {
    const exact = findModel(id);
    if (exact && exact.harness === harness) return exact;
    const slug = nativeIdFrom(id);
    const byNative = available.find(
      (model) => (model.nativeId ?? nativeIdFrom(model.id)) === slug,
    );
    if (byNative) return byNative;
    const prefix = available.find((model) => {
      const native = model.nativeId ?? nativeIdFrom(model.id);
      return native.startsWith(slug) || slug.startsWith(native);
    });
    if (prefix) return prefix;
  }
  const fallbackId = defaultModelId(harness);
  return (
    (fallbackId ? findModel(fallbackId) : undefined) ??
    available[0] ??
    MODELS.find((model) => model.harness === harness) ??
    MODELS[0]
  );
}

/** Catalog-reported context window for a model id, when known. */
export function modelContextWindow(id: string): number | undefined {
  const window = findModel(id)?.contextWindow;
  return window && window > 0 ? window : undefined;
}

export function nativeModelId(model: AgentModel | string): string {
  if (typeof model !== "string") {
    return model.nativeId ?? nativeIdFrom(model.id);
  }
  return findModel(model)?.nativeId ?? nativeIdFrom(model);
}

export function defaultModelSettings(
  model: AgentModel,
): Record<string, string> {
  const settings: Record<string, string> = {};
  for (const setting of model.settings ?? []) {
    settings[setting.id] = setting.value;
  }
  return settings;
}

export function mergeModelSettings(
  model: AgentModel,
  current?: Record<string, string>,
): Record<string, string> {
  const next = defaultModelSettings(model);
  if (!current) return next;
  for (const setting of model.settings ?? []) {
    const value = compatibleSettingValue(setting, current[setting.id]);
    if (value != null) next[setting.id] = value;
  }
  return next;
}

/** Last chosen effort/fast/etc., applied to any model that supports those values. */
export function preferredModelSettings(
  model: AgentModel,
  current?: Record<string, string>,
): Record<string, string> {
  return mergeModelSettings(model, {
    ...current,
    ...loadLastModelSettings(),
  });
}

export function loadLastModelSettings(): Record<string, string> {
  try {
    const raw = localStorage.getItem(LAST_MODEL_SETTINGS_KEY);
    if (!raw) return {};
    return parseStringRecord(JSON.parse(raw));
  } catch {
    return {};
  }
}

export function saveLastModelSettings(
  settings: Record<string, string>,
  mode: "overwrite" | "fill" = "overwrite",
) {
  const prev = loadLastModelSettings();
  const incoming = parseStringRecord(settings);
  const next =
    mode === "fill" ? { ...incoming, ...prev } : { ...prev, ...incoming };
  try {
    localStorage.setItem(LAST_MODEL_SETTINGS_KEY, JSON.stringify(next));
  } catch {
    // private mode / quota
  }
}

/** Compound launch id, e.g. `claude-opus-4-8[effort=high,fast=false]`. */
export function encodeModelLaunchId(
  modelId: string,
  settings?: Record<string, string>,
): string {
  const model = findModel(modelId);
  const native = nativeModelId(model ?? modelId);
  const defs = model?.settings ?? [];
  if (!native || defs.length === 0) return native;
  const parts = defs.map(
    (setting) => `${setting.id}=${settings?.[setting.id] ?? setting.value}`,
  );
  return `${native}[${parts.join(",")}]`;
}

export function loadFavoriteModels(): string[] {
  try {
    const raw = localStorage.getItem(FAVORITES_KEY);
    if (!raw) return [];
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed.filter((id): id is string => typeof id === "string");
  } catch {
    return [];
  }
}

export function saveFavoriteModels(ids: string[]) {
  try {
    localStorage.setItem(FAVORITES_KEY, JSON.stringify(ids));
  } catch {
    // private mode / quota
  }
}

function isHarnessId(value: string): value is HarnessId {
  return HARNESS_ORDER.includes(value as HarnessId);
}

export function loadModelPickerTab(): ModelPickerTab {
  try {
    const raw = localStorage.getItem(MODEL_PICKER_TAB_KEY);
    if (!raw) return "favorites";
    if (raw === "favorites") return "favorites";
    if (isHarnessId(raw)) return raw;
    return "favorites";
  } catch {
    return "favorites";
  }
}

export function saveModelPickerTab(tab: ModelPickerTab) {
  try {
    localStorage.setItem(MODEL_PICKER_TAB_KEY, tab);
  } catch {
    // private mode / quota
  }
}

export function modelPickerTabs(
  available: (id: HarnessId) => boolean,
): ModelPickerTab[] {
  return ["favorites", ...HARNESSES.filter(available)];
}

export function stepModelPickerTab(
  tab: ModelPickerTab,
  delta: -1 | 1,
  available: (id: HarnessId) => boolean,
): ModelPickerTab {
  const tabs = modelPickerTabs(available);
  if (tabs.length === 0) return tab;
  const index = tabs.indexOf(tab);
  const from = index < 0 ? 0 : index;
  return tabs[(from + delta + tabs.length) % tabs.length] ?? tab;
}

export function loadLastModelChoice(): LastModelChoice | null {
  try {
    const raw = localStorage.getItem(LAST_MODEL_KEY);
    if (!raw) return null;
    const parsed: unknown = JSON.parse(raw);
    if (
      typeof parsed === "object" &&
      parsed != null &&
      "harness" in parsed &&
      "model" in parsed &&
      typeof (parsed as LastModelChoice).harness === "string" &&
      typeof (parsed as LastModelChoice).model === "string" &&
      isHarnessId((parsed as LastModelChoice).harness)
    ) {
      return parsed as LastModelChoice;
    }
    return null;
  } catch {
    return null;
  }
}

export function saveLastModelChoice(harness: HarnessId, model: string) {
  try {
    localStorage.setItem(LAST_MODEL_KEY, JSON.stringify({ harness, model }));
  } catch {
    // private mode / quota
  }
}

function parseStringRecord(value: unknown): Record<string, string> {
  if (!value || typeof value !== "object" || Array.isArray(value)) return {};
  const out: Record<string, string> = {};
  for (const [key, entry] of Object.entries(value)) {
    if (typeof entry === "string") out[key] = entry;
  }
  return out;
}

/** Cursor CLI uses `extra-high`; Claude uses `xhigh`. */
const SETTING_VALUE_ALIASES: Record<string, string> = {
  "extra-high": "xhigh",
  xhigh: "extra-high",
};

function compatibleSettingValue(
  setting: ModelSetting,
  value: string | undefined,
): string | undefined {
  if (value == null) return undefined;
  if (setting.options.some((option) => option.value === value)) return value;
  const alias = SETTING_VALUE_ALIASES[value];
  if (alias && setting.options.some((option) => option.value === alias)) {
    return alias;
  }
  return undefined;
}

function nativeIdFrom(id: string): string {
  const trimmed = id.trim();
  const colon = trimmed.indexOf(":");
  const slug = colon >= 0 ? trimmed.slice(colon + 1) : trimmed;
  const bracket = slug.indexOf("[");
  return bracket >= 0 ? slug.slice(0, bracket) : slug;
}

function pickDefaultId(harness: HarnessId, models: AgentModel[]): string {
  if (harness === "claude") {
    return (
      models.find((model) => model.nativeId === "claude-sonnet-5")?.id ??
      models.find((model) => model.id === DEFAULT_MODEL_ID.claude)?.id ??
      models[0]?.id ??
      DEFAULT_MODEL_ID.claude
    );
  }
  if (harness === "cursor") {
    return (
      models.find((model) => model.nativeId === "composer-2.5")?.id ??
      models.find(
        (model) => model.nativeId === "default" || model.nativeId === "auto",
      )?.id ??
      models[0]?.id ??
      DEFAULT_MODEL_ID.cursor
    );
  }
  if (harness === "codex") {
    return models[0]?.id ?? "";
  }
  if (harness === "fx") {
    const preferred = [
      "zai/glm-5.2-fast",
      "zai/glm-5.2",
      "zai/glm-4.7-flash",
      "zai/glm-4.7",
      "openai/gpt-5.2",
    ];
    for (const nativeId of preferred) {
      const hit = models.find((model) => model.nativeId === nativeId);
      if (hit) return hit.id;
    }
    return (
      models.find((model) => model.id === DEFAULT_MODEL_ID.fx)?.id ??
      models[0]?.id ??
      DEFAULT_MODEL_ID.fx
    );
  }
  return (
    models.find((model) => model.id === DEFAULT_MODEL_ID[harness])?.id ??
    models[0]?.id ??
    DEFAULT_MODEL_ID[harness]
  );
}
