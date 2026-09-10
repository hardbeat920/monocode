import { describe, expect, it } from "vitest";
import {
  copyModelSettings,
  CUSTOM_MODEL_PRESETS,
  definitionFromDraft,
  draftFromDefinition,
  emptyEditorChoice,
  emptyEditorSetting,
  settingToEditor,
  validateCustomModelDraft,
  type CustomModelDraft,
} from "./customModelEditor";
import { readCustomModelEntries, toCustomModelSetting } from "./customModels";

const draft = (
  overrides: Partial<CustomModelDraft> = {},
): CustomModelDraft => ({
  slug: "private-model",
  name: "",
  settings: [],
  ...overrides,
});

describe("custom model editing", () => {
  it("round-trips names, choices and toggle defaults through storage", () => {
    const reasoning = settingToEditor(CUSTOM_MODEL_PRESETS.claude[0]);
    reasoning.choices = [
      { key: "a", value: " low ", label: "Low", isDefault: false },
      { key: "b", value: " max ", label: "", isDefault: true },
    ];
    const fast = {
      ...settingToEditor(CUSTOM_MODEL_PRESETS.claude[1]),
      enabled: true,
    };
    const edited = draft({ name: " My model ", settings: [reasoning, fast] });
    expect(validateCustomModelDraft(edited)).toBeNull();
    const definition = definitionFromDraft(edited);
    expect(definition).toMatchObject({
      name: "My model",
      settings: [
        {
          id: "effort",
          value: "max",
          options: [
            { value: "low", label: "Low" },
            { value: "max", label: "max" },
          ],
        },
        { id: "fast", value: "true" },
      ],
    });
    const [stored] = readCustomModelEntries([toCustomModelSetting(definition)]);
    expect(definitionFromDraft(draftFromDefinition(stored))).toEqual(
      definition,
    );
  });

  it("copies model options without Claude's built-in context and prompt mappings", () => {
    const settings = [
      {
        ...CUSTOM_MODEL_PRESETS.claude[0],
        options: [
          { value: "high", label: "High" },
          { value: "ultrathink", label: "Ultrathink" },
        ],
        value: "ultrathink",
        description: "Reasoning level",
      },
      {
        id: "context",
        label: "Context",
        kind: "select" as const,
        value: "1m",
        options: [{ value: "1m", label: "1M" }],
      },
      { ...CUSTOM_MODEL_PRESETS.claude[1], value: "true" },
    ];
    const copied = definitionFromDraft(
      draft({ settings: copyModelSettings(settings, "claude") }),
    );
    expect(copied.settings).toMatchObject([
      {
        id: "effort",
        value: "high",
        options: [{ value: "high", label: "High" }],
        description: "Reasoning level",
      },
      { id: "fast", value: "true" },
    ]);
    expect(settings[0].options).toHaveLength(2);
    const codex = definitionFromDraft(
      draft({
        settings: copyModelSettings(CUSTOM_MODEL_PRESETS.codex, "codex"),
      }),
    );
    expect(codex.settings).toEqual(CUSTOM_MODEL_PRESETS.codex);
  });

  it("uses the slug and provider options again when custom fields are cleared", () => {
    expect(
      toCustomModelSetting(definitionFromDraft(draft({ name: "  " }))),
    ).toBe("private-model");
  });

  it("requires unique option IDs, labels and nonempty choices", () => {
    const empty = emptyEditorSetting();
    expect(validateCustomModelDraft(draft({ settings: [empty] }))).toContain(
      "needs an ID",
    );
    empty.id = "effort";
    expect(validateCustomModelDraft(draft({ settings: [empty] }))).toContain(
      "needs a label",
    );
    empty.label = "Reasoning";
    expect(validateCustomModelDraft(draft({ settings: [empty] }))).toContain(
      "at least one choice",
    );
    empty.choices = [emptyEditorChoice()];
    expect(validateCustomModelDraft(draft({ settings: [empty] }))).toContain(
      "without a value",
    );
    empty.choices[0].value = "high";
    empty.choices.push({ ...emptyEditorChoice(), value: " high " });
    expect(validateCustomModelDraft(draft({ settings: [empty] }))).toContain(
      'choice "high" is used twice',
    );
    empty.choices.pop();
    expect(
      validateCustomModelDraft(
        draft({ settings: [empty, { ...empty, id: " effort " }] }),
      ),
    ).toContain('ID "effort" is used twice');
    expect(validateCustomModelDraft(draft({ settings: [empty] }))).toBeNull();
  });
});
