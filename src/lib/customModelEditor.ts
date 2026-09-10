// Adapted from T3 Code's customModelEditor.logic.ts. See NOTICE.
import type { CustomModelDefinition, CustomModelHarness } from "./customModels";
import type { ModelSetting } from "./models";

export type EditorChoice = {
  key: string;
  value: string;
  label: string;
  isDefault: boolean;
};

export type EditorSetting = {
  key: string;
  id: string;
  label: string;
  kind: "select" | "toggle";
  choices: EditorChoice[];
  enabled?: boolean;
  description?: string;
};

export type CustomModelDraft = {
  slug: string;
  name: string;
  settings: EditorSetting[];
};

const effortChoices = [
  { value: "low", label: "Low" },
  { value: "medium", label: "Medium" },
  { value: "high", label: "High" },
  { value: "xhigh", label: "Extra High" },
];

const toggleChoices = [
  { value: "true", label: "On" },
  { value: "false", label: "Off" },
];

/** IDs and values are the ones MonoCode's adapters read from each turn. */
export const CUSTOM_MODEL_PRESETS: Record<CustomModelHarness, ModelSetting[]> =
  {
    codex: [
      {
        id: "reasoningEffort",
        label: "Reasoning",
        kind: "select",
        value: "medium",
        options: effortChoices,
      },
      {
        id: "serviceTier",
        label: "Speed",
        kind: "select",
        value: "default",
        options: [
          { value: "default", label: "Standard" },
          { value: "fast", label: "Fast" },
        ],
      },
    ],
    claude: [
      {
        id: "effort",
        label: "Reasoning",
        kind: "select",
        value: "high",
        options: [...effortChoices, { value: "max", label: "Max" }],
      },
      {
        id: "fast",
        label: "Fast mode",
        kind: "toggle",
        value: "false",
        options: toggleChoices,
      },
      {
        id: "thinking",
        label: "Thinking",
        kind: "toggle",
        value: "false",
        options: toggleChoices,
      },
    ],
  };

let nextKey = 0;
function newKey(): string {
  return `custom-model-${++nextKey}`;
}

export function emptyEditorChoice(): EditorChoice {
  return { key: newKey(), value: "", label: "", isDefault: false };
}

export function emptyEditorSetting(): EditorSetting {
  return { key: newKey(), id: "", label: "", kind: "select", choices: [] };
}

export function settingToEditor(setting: ModelSetting): EditorSetting {
  return {
    key: newKey(),
    id: setting.id,
    label: setting.label,
    kind: setting.kind,
    enabled: setting.value === "true",
    description: setting.description,
    choices:
      setting.kind === "select"
        ? setting.options.map((option) => ({
            key: newKey(),
            ...option,
            isDefault: option.value === setting.value,
          }))
        : [],
  };
}

export function draftFromDefinition(
  entry: CustomModelDefinition,
): CustomModelDraft {
  return {
    slug: entry.slug,
    name: entry.name === entry.slug ? "" : entry.name,
    settings: (entry.settings ?? []).map(settingToEditor),
  };
}

/** Custom Claude models do not inherit built-in context or prompt mappings. */
export function copyModelSettings(
  settings: ModelSetting[],
  harness: CustomModelHarness,
): EditorSetting[] {
  return settings
    .filter((setting) => harness !== "claude" || setting.id !== "context")
    .map((setting) => {
      if (harness !== "claude" || setting.id !== "effort")
        return settingToEditor(setting);
      const options = setting.options.filter(
        (option) => option.value !== "ultrathink",
      );
      return settingToEditor({
        ...setting,
        options,
        value: options.some((option) => option.value === setting.value)
          ? setting.value
          : (options[0]?.value ?? ""),
      });
    });
}

export function validateCustomModelDraft(
  draft: CustomModelDraft,
): string | null {
  const seen = new Set<string>();
  for (const [index, setting] of draft.settings.entries()) {
    const position = `Option ${index + 1}`;
    const id = setting.id.trim();
    if (!id) return `${position} needs an ID.`;
    if (seen.has(id)) return `${position}: ID "${id}" is used twice.`;
    seen.add(id);
    if (!setting.label.trim()) return `${position} needs a label.`;
    if (setting.kind !== "select") continue;
    if (setting.choices.length === 0)
      return `${position} needs at least one choice.`;
    const choices = new Set<string>();
    for (const choice of setting.choices) {
      const value = choice.value.trim();
      if (!value) return `${position} has a choice without a value.`;
      if (choices.has(value))
        return `${position}: choice "${value}" is used twice.`;
      choices.add(value);
    }
  }
  return null;
}

export function definitionFromDraft(
  draft: CustomModelDraft,
): CustomModelDefinition {
  const settings = draft.settings.map((setting): ModelSetting => {
    const options =
      setting.kind === "toggle"
        ? toggleChoices
        : setting.choices.map((choice) => ({
            value: choice.value.trim(),
            label: choice.label.trim() || choice.value.trim(),
          }));
    const value =
      setting.kind === "toggle"
        ? String(setting.enabled ?? false)
        : (setting.choices.find((choice) => choice.isDefault)?.value.trim() ??
          options[0]?.value ??
          "");
    return {
      id: setting.id.trim(),
      label: setting.label.trim(),
      kind: setting.kind,
      value,
      options,
      ...(setting.description !== undefined
        ? { description: setting.description }
        : {}),
    };
  });
  return {
    slug: draft.slug,
    name: draft.name.trim() || draft.slug,
    settings: settings.length > 0 ? settings : null,
  };
}
