import { describe, expect, it } from "vitest";
import { modelsFromClaudeListModels } from "./claudeCatalog";

const row = (value: string, extra: Record<string, unknown> = {}) => ({
  value,
  displayName: value,
  resolvedModel: `claude-${value}`,
  ...extra,
});

describe("modelsFromClaudeListModels supportsAutoMode", () => {
  it("maps true and false, and absent as unsupported", () => {
    const models = modelsFromClaudeListModels([
      row("opus-x", { supportsAutoMode: true }),
      row("haiku-x", { supportsAutoMode: false }),
      row("sonnet-x"),
    ]);
    const byName = (name: string) =>
      models.find((model) => model.name.toLowerCase().includes(name));
    expect(byName("opus")?.supportsAuto).toBe(true);
    expect(byName("haiku")?.supportsAuto).toBe(false);
    expect(byName("sonnet")?.supportsAuto).toBe(false);
  });
});
