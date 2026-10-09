import { afterEach, describe, expect, it } from "vitest";
import { modelsFromClaudeListModels } from "./claudeCatalog";
import {
  nativeModelId,
  resolveModel,
  resetHarnessModelOverlays,
  setHarnessModels,
} from "../../../../features/sessions/model/models";

afterEach(resetHarnessModelOverlays);

describe("release catalog regression probes", () => {
  it("retains a saved gateway model while another profile's catalog is active", () => {
    const model = resolveModel(
      "claude",
      "claude:us.anthropic.claude-sonnet-4-5-v1.0",
    );
    expect(model.id).toBe("claude:us.anthropic.claude-sonnet-4-5-v1.0");
    expect(nativeModelId(model)).toBe("us.anthropic.claude-sonnet-4-5-v1.0");
  });
  it("preserves dotted cloud model IDs before discovery", () => {
    expect(nativeModelId("claude:us.anthropic.claude-sonnet-4-5-v1.0")).toBe(
      "us.anthropic.claude-sonnet-4-5-v1.0",
    );
  });
  it("preserves a custom gateway ID advertised by the installed CLI", () => {
    const rows = [
      {
        value: "my-gateway/claude-opus-5-5",
        displayName: "Audit custom gateway",
      },
    ];
    const model = modelsFromClaudeListModels(rows).find(
      (row) => row.name === "Audit custom gateway",
    );
    expect(model?.nativeId).toBe("my-gateway/claude-opus-5-5");
  });

  it("passes a provider model ID through without adding claude-", () => {
    setHarnessModels("claude", [
      {
        id: "claude:my-gateway/claude-opus-5-5",
        harness: "claude",
        name: "Gateway",
        nativeId: "my-gateway/claude-opus-5-5",
      },
    ]);
    expect(nativeModelId("claude:my-gateway/claude-opus-5-5")).toBe(
      "my-gateway/claude-opus-5-5",
    );
  });

  it("retains the 1M option when base and extended context rows coexist", () => {
    const rows = modelsFromClaudeListModels([
      { value: "opus", resolvedModel: "claude-opus-4-6", displayName: "Opus" },
      {
        value: "opus[1m]",
        resolvedModel: "claude-opus-4-6",
        displayName: "Opus 1M",
      },
    ]);
    const options =
      rows[0].settings
        ?.find((setting) => setting.id === "context")
        ?.options.map((option) => option.value) ?? [];
    expect(options).toContain("1m");
  });
});
