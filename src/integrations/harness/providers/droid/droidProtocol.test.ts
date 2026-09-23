import { describe, expect, it } from "vitest";
import {
  droidConfigOptionsFrom,
  droidCurrentModelId,
  droidEffortConfig,
  droidEffortSetting,
  droidEffortValue,
  droidErrorMessage,
  droidModeId,
  droidSpecPlan,
  droidStartupError,
  isDroidErrorEcho,
  modelsFromDroidSession,
  type DroidConfigOption,
} from "./droidProtocol";

// Trimmed from a real `droid exec --output-format acp` session/new (0.225.1).
const SESSION_NEW = {
  sessionId: "06480ace-4dc2-480f-9b83-950224e10c22",
  models: {
    availableModels: [
      { modelId: "auto", name: "Auto Model", description: "1x Factory token rate" },
      { modelId: "claude-opus-5-5", name: "Opus 5.5" },
      { modelId: "gpt-6-luna", name: "GPT-6 Luna" },
    ],
    currentModelId: "gpt-6-luna",
  },
  modes: {
    availableModes: [
      { id: "normal", name: "Auto (Off)" },
      { id: "spec", name: "Spec" },
    ],
    currentModeId: "normal",
  },
  configOptions: [
    {
      id: "autonomy_level",
      category: "mode",
      type: "select",
      currentValue: "normal",
      options: [
        { value: "normal", name: "Auto (Off)" },
        { value: "auto-high", name: "Auto (High)" },
      ],
    },
    {
      id: "model",
      category: "model",
      type: "select",
      currentValue: "gpt-6-luna",
      options: [
        { value: "auto", name: "Auto Model" },
        { value: "claude-opus-5-5", name: "Opus 5.5" },
        { value: "gpt-6-luna", name: "GPT-6 Luna" },
      ],
    },
    {
      id: "reasoning_effort",
      category: "thought_level",
      type: "select",
      currentValue: "medium",
      options: [
        { value: "none", name: "None" },
        { value: "low", name: "Low" },
        { value: "medium", name: "Medium" },
        { value: "xhigh", name: "Extra High" },
      ],
    },
  ],
};

describe("Factory Droid ACP protocol", () => {
  it("maps access modes onto Droid autonomy levels", () => {
    expect(droidModeId("supervised")).toBe("normal");
    expect(droidModeId("auto-accept-edits")).toBe("auto-low");
    expect(droidModeId("auto")).toBe("auto-medium");
    expect(droidModeId("full-access")).toBe("auto-high");
    expect(droidModeId("full-access", true)).toBe("spec");
  });

  it("reads the catalog with Droid's current model first", () => {
    const models = modelsFromDroidSession(SESSION_NEW);
    expect(models.map((model) => model.id)).toEqual([
      "droid:gpt-6-luna",
      "droid:auto",
      "droid:claude-opus-5-5",
    ]);
    expect(models[0]).toMatchObject({
      harness: "droid",
      name: "GPT-6 Luna",
      nativeId: "gpt-6-luna",
    });
    expect(droidCurrentModelId(SESSION_NEW)).toBe("gpt-6-luna");
  });

  it("attaches per-model reasoning levels as the effort setting", () => {
    const opus: DroidConfigOption = {
      id: "reasoning_effort",
      category: "thought_level",
      currentValue: "high",
      options: [
        { value: "low", name: "Low" },
        { value: "high", name: "High" },
        { value: "max", name: "Maximum" },
      ],
    };
    const auto: DroidConfigOption = {
      id: "reasoning_effort",
      currentValue: "none",
      options: [{ value: "none", name: "None" }],
    };
    const models = modelsFromDroidSession(
      SESSION_NEW,
      new Map([
        ["claude-opus-5-5", opus],
        ["auto", auto],
      ]),
    );
    const byId = new Map(models.map((model) => [model.nativeId, model]));
    expect(byId.get("claude-opus-5-5")?.settings).toEqual([
      {
        id: "effort",
        label: "Reasoning",
        kind: "select",
        value: "high",
        options: [
          { value: "low", label: "Low" },
          { value: "high", label: "High" },
          { value: "max", label: "Maximum" },
        ],
      },
    ]);
    // A single fixed level is not a choice.
    expect(byId.get("auto")?.settings).toBeUndefined();
    expect(droidEffortSetting(undefined)).toBeUndefined();
  });

  it("reads config options from results and config_option_update", () => {
    const fromResult = droidConfigOptionsFrom(SESSION_NEW);
    expect(droidEffortConfig(fromResult ?? [])?.currentValue).toBe("medium");
    const fromUpdate = droidConfigOptionsFrom({
      sessionId: "s",
      update: {
        sessionUpdate: "config_option_update",
        configOptions: SESSION_NEW.configOptions,
      },
    });
    expect(fromUpdate?.map((option) => option.id)).toEqual([
      "autonomy_level",
      "model",
      "reasoning_effort",
    ]);
    expect(droidConfigOptionsFrom({ update: { sessionUpdate: "x" } })).toBeNull();
  });

  it("maps MonoCode effort values onto the model's levels", () => {
    const config = droidEffortConfig(droidConfigOptionsFrom(SESSION_NEW) ?? []);
    expect(droidEffortValue(config, { effort: "low" })).toBe("low");
    expect(droidEffortValue(config, { effort: "extra-high" })).toBe("xhigh");
    expect(droidEffortValue(config, { effort: "off" })).toBe("none");
    expect(droidEffortValue(config, { effort: "max" })).toBeUndefined();
    expect(droidEffortValue(config, {})).toBeUndefined();
  });

  it("surfaces the detail Droid hides in JSON-RPC error data", () => {
    const error = Object.assign(new Error("Internal error: Agent error"), {
      code: -32603,
      data: '402 {"detail":"You\'ve reached your 5-hour Droid Core usage limit (resets in 2h 15min).","status":402}',
    });
    expect(droidErrorMessage(error)).toBe(
      "You've reached your 5-hour Droid Core usage limit (resets in 2h 15min).",
    );
    expect(droidErrorMessage(new Error("plain"))).toBe("plain");
    expect(
      droidStartupError(new Error("Authentication required")).message,
    ).toContain("FACTORY_API_KEY");
  });

  it("drops the streamed echo of a failed request", () => {
    expect(
      isDroidErrorEcho({
        update: {
          sessionUpdate: "agent_message_chunk",
          content: { type: "text", text: 'Error: 402 {"detail":"limit"}' },
        },
      }),
    ).toBe(true);
    expect(
      isDroidErrorEcho({
        update: {
          sessionUpdate: "agent_message_chunk",
          content: { type: "text", text: "Error: handled gracefully" },
        },
      }),
    ).toBe(false);
  });

  it("extracts the spec from an exit-spec permission request", () => {
    expect(
      droidSpecPlan({
        toolCall: {
          toolCallId: "t1",
          kind: "switch_mode",
          title: "Exit spec mode",
          rawInput: { plan: "## Plan\n1. Do it" },
        },
      }),
    ).toBe("## Plan\n1. Do it");
    expect(
      droidSpecPlan({
        toolCall: {
          toolCallId: "t2",
          kind: "switch_mode",
          content: [{ type: "content", content: { type: "text", text: "Spec body" } }],
        },
      }),
    ).toBe("Spec body");
    expect(
      droidSpecPlan({ toolCall: { toolCallId: "t3", kind: "edit" } }),
    ).toBeUndefined();
  });
});
