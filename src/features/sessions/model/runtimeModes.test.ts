import { afterEach, describe, expect, it } from "vitest";
import {
  coerceRuntimeMode,
  resetHarnessModelOverlays,
  runtimeModesFor,
  setHarnessModels,
} from "./models";

afterEach(() => resetHarnessModelOverlays());

const confirmAuto = (supportsAuto: boolean) =>
  setHarnessModels("claude", [
    { id: "claude:sonnet-5", harness: "claude", name: "S", supportsAuto },
  ]);

describe("runtimeModesFor", () => {
  it("offers Auto for Claude and Codex models", () => {
    confirmAuto(true);
    expect(runtimeModesFor("claude", "claude:sonnet-5")).toContain("auto");
    expect(runtimeModesFor("codex", "codex:gpt-5")).toContain("auto");
  });

  it("hides Auto for other harnesses", () => {
    expect(runtimeModesFor("cursor", "cursor:default")).toEqual([
      "supervised",
      "auto-accept-edits",
      "full-access",
    ]);
  });

  it("offers only what each provider honors", () => {
    const edits = ["supervised", "auto-accept-edits", "full-access"];
    expect(runtimeModesFor("grok", "grok:grok-4.6")).toContain("auto");
    for (const harness of [
      "cursor",
      "opencode",
      "antigravity",
      "hermes",
    ] as const) {
      expect(runtimeModesFor(harness, `${harness}:x`)).toEqual(edits);
    }
    for (const harness of ["pi", "omp", "fx"] as const) {
      expect(runtimeModesFor(harness, `${harness}:x`)).toEqual([]);
    }
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

  it("treats unconfirmed Claude support as unsupported until discovery says otherwise", () => {
    expect(runtimeModesFor("claude", "claude:sonnet-5")).not.toContain("auto");
    expect(coerceRuntimeMode("claude", "claude:sonnet-5", "auto")).toBe(
      "auto-accept-edits",
    );
    confirmAuto(true);
    expect(coerceRuntimeMode("claude", "claude:sonnet-5", "auto")).toBe("auto");
  });

  it("steps down to the previous mode when Auto isn't offered", () => {
    confirmAuto(true);
    expect(coerceRuntimeMode("cursor", "cursor:default", "auto")).toBe(
      "auto-accept-edits",
    );
    expect(coerceRuntimeMode("claude", "claude:sonnet-5", "auto")).toBe("auto");
    expect(coerceRuntimeMode("cursor", "cursor:default", "full-access")).toBe(
      "full-access",
    );
  });
});
