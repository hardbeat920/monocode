import { describe, expect, it } from "vitest";

import {
  effortSetting,
  fastSetting,
  otherSettings,
  withModel,
} from "@/features/models/modelChoice";
import type { CatalogModel, ModelChoice } from "@/features/models/types";

const effort = {
  id: "effort",
  label: "Effort",
  kind: "select" as const,
  value: "high",
  options: ["low", "medium", "high", "max"].map((value) => ({ value, label: value })),
};
const fast = {
  id: "fast",
  label: "Fast",
  kind: "toggle" as const,
  value: "false",
  options: [
    { value: "true", label: "On" },
    { value: "false", label: "Off" },
  ],
};
const opus: CatalogModel = { id: "claude:opus", harness: "claude", name: "Opus", settings: [effort, fast] };
const sonnet: CatalogModel = {
  id: "claude:sonnet",
  harness: "claude",
  name: "Sonnet",
  settings: [{ ...effort, options: effort.options.slice(0, 3) }],
};

describe("model choice", () => {
  it("finds effort and fast mode as the desktop does", () => {
    expect(effortSetting(opus)?.id).toBe("effort");
    expect(fastSetting(opus)).toMatchObject({ on: "true", off: "false" });
    expect(otherSettings(opus)).toEqual([]);
    const codex: CatalogModel = {
      id: "codex:gpt",
      harness: "codex",
      name: "GPT",
      settings: [
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
    };
    expect(fastSetting(codex)).toMatchObject({ on: "fast", off: "default" });
  });

  it("carries settings to another model where they still apply", () => {
    const choice: ModelChoice = {
      harness: "claude",
      model: "claude:opus",
      modelSettings: { effort: "max", fast: "true" },
      runtimeMode: "supervised",
    };
    expect(withModel(choice, sonnet).modelSettings).toEqual({ effort: "high" });
    expect(
      withModel({ ...choice, modelSettings: { effort: "low" } }, sonnet).modelSettings,
    ).toEqual({ effort: "low" });
  });
});
