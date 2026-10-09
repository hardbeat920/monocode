import { afterEach, describe, expect, it } from "vitest";
import {
  coerceRuntimeMode,
  modelSupportsAuto,
  resetHarnessModelOverlays,
  runtimeModesFor,
  setHarnessModels,
} from "./models";

afterEach(() => resetHarnessModelOverlays());

describe("runtimeModesFor", () => {
  it("offers Auto for Claude and Codex models", () => {
    expect(runtimeModesFor("claude", "claude:sonnet-5")).toContain("auto");
    expect(modelSupportsAuto("codex", "codex:gpt-5")).toBe(true);
  });

  it("hides Auto for other harnesses", () => {
    expect(runtimeModesFor("cursor", "cursor:default")).toEqual([
      "supervised",
      "auto-accept-edits",
      "full-access",
    ]);
  });

  it("hides Auto for a model that reports no support", () => {
    setHarnessModels("claude", [
      {
        id: "claude:haiku",
        harness: "claude",
        name: "Haiku",
        supportsAuto: false,
      },
      {
        id: "claude:opus",
        harness: "claude",
        name: "Opus",
        supportsAuto: true,
      },
    ]);
    expect(runtimeModesFor("claude", "claude:haiku")).not.toContain("auto");
    expect(runtimeModesFor("claude", "claude:opus")).toContain("auto");
  });

  it("steps down to the previous mode when Auto isn't offered", () => {
    expect(coerceRuntimeMode("cursor", "cursor:default", "auto")).toBe(
      "auto-accept-edits",
    );
    expect(coerceRuntimeMode("claude", "claude:sonnet-5", "auto")).toBe("auto");
    expect(coerceRuntimeMode("cursor", "cursor:default", "full-access")).toBe(
      "full-access",
    );
  });
});
