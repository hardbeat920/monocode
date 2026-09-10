// Custom model editing is adapted from T3 Code; see NOTICE.
import { useId, useState } from "react";
import {
  copyModelSettings,
  CUSTOM_MODEL_PRESETS,
  definitionFromDraft,
  draftFromDefinition,
  emptyEditorChoice,
  emptyEditorSetting,
  settingToEditor,
  validateCustomModelDraft,
  type EditorChoice,
  type EditorSetting,
} from "../lib/customModelEditor";
import type {
  CustomModelDefinition,
  CustomModelHarness,
} from "../lib/customModels";
import type { AgentModel } from "../lib/models";
import { Plus, X } from "./icons";

const inputClass =
  "min-w-0 rounded-md border border-content/10 bg-content/5 px-2 py-1.5 text-[12px] text-content outline-none placeholder:text-content/30 focus:border-content/30";
const buttonClass =
  "flex items-center justify-center gap-1.5 rounded-md border border-content/10 px-2.5 py-1 text-[12px] text-content/70 hover:bg-content/10 hover:text-content focus-visible:outline focus-visible:outline-accent";
const iconButtonClass =
  "grid size-6 shrink-0 place-items-center rounded-md text-content/40 hover:bg-content/10 hover:text-content focus-visible:outline focus-visible:outline-accent";
const CUSTOM_OPTION = "__custom__";

export function CustomModelEditor({
  harness,
  entry,
  builtInModels,
  onSave,
  onCancel,
}: {
  harness: CustomModelHarness;
  entry: CustomModelDefinition;
  builtInModels: AgentModel[];
  onSave: (entry: CustomModelDefinition) => string | null;
  onCancel: () => void;
}) {
  const id = useId();
  const [draft, setDraft] = useState(() => draftFromDefinition(entry));
  const [error, setError] = useState<string | null>(null);
  const presets = CUSTOM_MODEL_PRESETS[harness];
  const candidates = builtInModels.filter((model) => model.settings?.length);

  const updateSetting = (key: string, patch: Partial<EditorSetting>) => {
    setError(null);
    setDraft((current) => ({
      ...current,
      settings: current.settings.map((setting) =>
        setting.key === key ? { ...setting, ...patch } : setting,
      ),
    }));
  };

  const updateChoice = (
    settingKey: string,
    choiceKey: string,
    patch: Partial<EditorChoice>,
  ) => {
    setError(null);
    setDraft((current) => ({
      ...current,
      settings: current.settings.map((setting) =>
        setting.key !== settingKey
          ? setting
          : {
              ...setting,
              choices: setting.choices.map((choice) =>
                choice.key === choiceKey
                  ? { ...choice, ...patch }
                  : patch.isDefault
                    ? { ...choice, isDefault: false }
                    : choice,
              ),
            },
      ),
    }));
  };

  const addSetting = (setting: EditorSetting) => {
    setError(null);
    setDraft((current) => ({
      ...current,
      settings: [...current.settings, setting],
    }));
  };

  return (
    <form
      data-custom-model-editor
      aria-label={`Edit ${entry.name}`}
      className="mt-2 space-y-4 rounded-lg border border-content/10 bg-content/3 p-3"
      onKeyDown={(event) => {
        if (event.key !== "Escape" || event.nativeEvent.isComposing) return;
        event.preventDefault();
        event.stopPropagation();
        onCancel();
      }}
      onSubmit={(event) => {
        event.preventDefault();
        const problem = validateCustomModelDraft(draft);
        setError(problem ?? onSave(definitionFromDraft(draft)));
      }}
    >
      <label className="flex max-w-sm flex-col gap-1.5 text-[12px] text-content/50">
        Display name
        <input
          autoFocus
          value={draft.name}
          placeholder={draft.slug}
          className={inputClass}
          onChange={(event) =>
            setDraft((current) => ({ ...current, name: event.target.value }))
          }
        />
      </label>

      <div className="space-y-2.5">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <span className="text-[12px] text-content/50">
            Options in the composer
          </span>
          {candidates.length > 0 ? (
            <select
              aria-label="Copy options from a built-in model"
              className={`${inputClass} max-w-52`}
              value=""
              onChange={(event) => {
                const model = candidates.find(
                  (candidate) => candidate.id === event.target.value,
                );
                if (!model?.settings) return;
                setError(null);
                setDraft((current) => ({
                  ...current,
                  settings: copyModelSettings(model.settings!, harness),
                }));
              }}
            >
              <option value="" disabled>
                Copy from a model…
              </option>
              {candidates.map((model) => (
                <option key={model.id} value={model.id}>
                  {model.name}
                </option>
              ))}
            </select>
          ) : null}
        </div>

        {draft.settings.length === 0 ? (
          <p className="text-[12px] leading-relaxed text-content/40">
            {harness === "codex"
              ? "Uses the options from the Codex catalog until you add your own."
              : "Add the options your model supports, or leave it with no extra options."}
          </p>
        ) : null}

        {draft.settings.map((setting, index) => {
          const presetId = presets.some((preset) => preset.id === setting.id)
            ? setting.id
            : CUSTOM_OPTION;
          return (
            <fieldset
              key={setting.key}
              className="min-w-0 space-y-2 rounded-md border border-content/10 p-2.5"
            >
              <legend className="px-1 text-[11px] text-content/40">
                Option {index + 1}
              </legend>
              <div className="flex flex-wrap items-center gap-2">
                <select
                  aria-label={`Option ${index + 1} ID`}
                  value={presetId}
                  className={`${inputClass} max-w-44`}
                  onChange={(event) => {
                    if (event.target.value === CUSTOM_OPTION) {
                      updateSetting(setting.key, { id: "" });
                      return;
                    }
                    const preset = presets.find(
                      (candidate) => candidate.id === event.target.value,
                    );
                    if (preset)
                      updateSetting(setting.key, {
                        ...settingToEditor(preset),
                        key: setting.key,
                      });
                  }}
                >
                  {presets.map((preset) => (
                    <option key={preset.id} value={preset.id}>
                      {preset.label}
                    </option>
                  ))}
                  <option value={CUSTOM_OPTION}>Custom option…</option>
                </select>
                {presetId === CUSTOM_OPTION ? (
                  <input
                    aria-label={`Option ${index + 1} custom ID`}
                    value={setting.id}
                    spellCheck={false}
                    placeholder="optionId"
                    className={`${inputClass} w-32 font-mono`}
                    onChange={(event) =>
                      updateSetting(setting.key, { id: event.target.value })
                    }
                  />
                ) : null}
                <input
                  aria-label={`Option ${index + 1} label`}
                  value={setting.label}
                  placeholder="Label"
                  className={`${inputClass} w-28 flex-1`}
                  onChange={(event) =>
                    updateSetting(setting.key, { label: event.target.value })
                  }
                />
                <select
                  aria-label={`Option ${index + 1} type`}
                  value={setting.kind}
                  className={inputClass}
                  onChange={(event) =>
                    updateSetting(setting.key, {
                      kind:
                        event.target.value === "toggle" ? "toggle" : "select",
                    })
                  }
                >
                  <option value="select">Choices</option>
                  <option value="toggle">Toggle</option>
                </select>
                <button
                  type="button"
                  aria-label={`Remove option ${index + 1}`}
                  className={iconButtonClass}
                  onClick={() => {
                    setError(null);
                    setDraft((current) => ({
                      ...current,
                      settings: current.settings.filter(
                        (candidate) => candidate.key !== setting.key,
                      ),
                    }));
                  }}
                >
                  <X className="size-3.5" />
                </button>
              </div>

              {setting.kind === "toggle" ? (
                <label className="flex items-center gap-2 text-[12px] text-content/50">
                  <input
                    type="checkbox"
                    className="accent-accent"
                    checked={setting.enabled ?? false}
                    onChange={(event) =>
                      updateSetting(setting.key, {
                        enabled: event.target.checked,
                      })
                    }
                  />
                  On by default
                </label>
              ) : (
                <div className="space-y-1.5">
                  {setting.choices.map((choice, choiceIndex) => (
                    <div
                      key={choice.key}
                      className="flex flex-wrap items-center gap-2"
                    >
                      <input
                        aria-label={`Option ${index + 1} choice ${choiceIndex + 1} value`}
                        value={choice.value}
                        placeholder="value"
                        spellCheck={false}
                        className={`${inputClass} w-24 flex-1 font-mono`}
                        onChange={(event) =>
                          updateChoice(setting.key, choice.key, {
                            value: event.target.value,
                          })
                        }
                      />
                      <input
                        aria-label={`Option ${index + 1} choice ${choiceIndex + 1} label`}
                        value={choice.label}
                        placeholder="Label"
                        className={`${inputClass} w-28 flex-1`}
                        onChange={(event) =>
                          updateChoice(setting.key, choice.key, {
                            label: event.target.value,
                          })
                        }
                      />
                      <label className="flex items-center gap-1.5 text-[11px] text-content/50">
                        <input
                          type="radio"
                          className="accent-accent"
                          name={`${id}-${setting.key}-default`}
                          checked={choice.isDefault}
                          onChange={() =>
                            updateChoice(setting.key, choice.key, {
                              isDefault: true,
                            })
                          }
                        />
                        Default
                      </label>
                      <button
                        type="button"
                        aria-label={`Remove option ${index + 1} choice ${choiceIndex + 1}`}
                        className={iconButtonClass}
                        onClick={() =>
                          updateSetting(setting.key, {
                            choices: setting.choices.filter(
                              (candidate) => candidate.key !== choice.key,
                            ),
                          })
                        }
                      >
                        <X className="size-3.5" />
                      </button>
                    </div>
                  ))}
                  <button
                    type="button"
                    className={buttonClass}
                    onClick={() =>
                      updateSetting(setting.key, {
                        choices: [...setting.choices, emptyEditorChoice()],
                      })
                    }
                  >
                    <Plus className="size-3" />
                    Add choice
                  </button>
                </div>
              )}
            </fieldset>
          );
        })}

        <div className="flex flex-wrap gap-2">
          {presets
            .filter(
              (preset) =>
                !draft.settings.some((setting) => setting.id === preset.id),
            )
            .map((preset) => (
              <button
                key={preset.id}
                type="button"
                className={buttonClass}
                onClick={() => addSetting(settingToEditor(preset))}
              >
                <Plus className="size-3" />
                {preset.label}
              </button>
            ))}
          <button
            type="button"
            className={buttonClass}
            onClick={() => addSetting(emptyEditorSetting())}
          >
            <Plus className="size-3" />
            Custom option
          </button>
        </div>
      </div>

      {error ? (
        <p role="alert" className="text-[12px] text-red-400">
          {error}
        </p>
      ) : null}
      <div className="flex gap-2">
        <button
          type="submit"
          className={`${buttonClass} bg-content/10 text-content`}
        >
          Save model
        </button>
        <button type="button" className={buttonClass} onClick={onCancel}>
          Cancel
        </button>
      </div>
    </form>
  );
}
