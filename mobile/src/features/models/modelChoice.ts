import type { CatalogModel, ModelPick, ModelSetting } from "./types";

// The desktop's reading of a model's settings (EFFORT_SETTING_IDS and
// modelEffortSetting in models.ts, the fast toggle in QuickModelSelector).

const EFFORT_IDS = new Set(["effort", "reasoning", "reasoningEffort", "thinking", "variant"]);

/** The setting the effort slider moves, if the model has one. */
export function effortSetting(model: CatalogModel | undefined): ModelSetting | undefined {
  return model?.settings.find(
    (setting) =>
      setting.kind === "select" &&
      setting.options.length > 1 &&
      (EFFORT_IDS.has(setting.id) || /^(effort|reasoning effort|reasoning)$/i.test(setting.label)),
  );
}

/** Fast mode, as a toggle or as Codex's service tier, with its on and off values. */
export function fastSetting(
  model: CatalogModel | undefined,
): { setting: ModelSetting; on: string; off: string } | undefined {
  const setting = model?.settings.find(
    (setting) =>
      setting.id === "fast" || setting.id === "serviceTier" || /^fast( mode)?$/i.test(setting.label),
  );
  if (!setting) return undefined;
  const on = setting.options.find((option) => /^(true|fast|priority)$/i.test(option.value));
  const off = setting.options.find((option) => /^(false|default|standard|normal)$/i.test(option.value));
  return on && off ? { setting, on: on.value, off: off.value } : undefined;
}

/** The model's other settings, shown as plain choices under effort and fast mode. */
export function otherSettings(model: CatalogModel | undefined): ModelSetting[] {
  const effort = effortSetting(model);
  const fast = fastSetting(model)?.setting;
  return (model?.settings ?? []).filter(
    (setting) => setting !== effort && setting !== fast && setting.options.length > 1,
  );
}

/**
 * Settings for `model`, keeping each current value it also offers and its
 * default for the rest, as the desktop's mergeModelSettings does.
 */
export function settingsFor(
  model: CatalogModel,
  current: Record<string, string> = {},
): Record<string, string> {
  return Object.fromEntries(
    model.settings.map((setting) => [
      setting.id,
      setting.options.some((option) => option.value === current[setting.id])
        ? current[setting.id]
        : setting.value,
    ]),
  );
}

/** The choice with another model, its settings carried over where they still apply. */
export function withModel<T extends ModelPick>(choice: T, model: CatalogModel): T {
  return {
    ...choice,
    harness: model.harness,
    model: model.id,
    modelSettings: settingsFor(model, choice.modelSettings),
  };
}

/** The label of the effort the choice runs `model` at, if it has an effort. */
export function effortLabel(model: CatalogModel | undefined, choice: ModelPick) {
  const setting = effortSetting(model);
  if (!setting) return undefined;
  const value = settingValue(choice, setting);
  return setting.options.find((option) => option.value === value)?.label ?? value;
}

/** The value a setting has in the choice, or its default. */
export function settingValue(choice: ModelPick, setting: ModelSetting) {
  return choice.modelSettings[setting.id] ?? setting.value;
}
