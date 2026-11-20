import { afterEach, describe, expect, it } from "vitest";
import {
  coerceRuntimeMode,
  resetHarnessModelOverlays,
  runtimeModesFor,
  setHarnessModels,
  type AgentModel,
  type ModelCatalog,
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
    expect(runtimeModesFor("devin", "devin:x")).toContain("auto");
    for (const harness of [
      "cursor",
      "copilot",
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
    expect(coerceRuntimeMode("copilot", "copilot:auto", "auto")).toBe(
      "auto-accept-edits",
    );
    expect(coerceRuntimeMode("claude", "claude:sonnet-5", "auto")).toBe("auto");
    expect(coerceRuntimeMode("cursor", "cursor:default", "full-access")).toBe(
      "full-access",
    );
  });
});

describe("saved model ids that differ from discovered ones", () => {
  const alias: AgentModel = {
    id: "claude:opus",
    harness: "claude",
    name: "Opus 5.5",
    nativeId: "opus",
    aliasTarget: "claude-opus-5-5",
    supportsAuto: true,
  };

  it("keeps Auto for a saved concrete model advertised through an alias", () => {
    setHarnessModels("claude", [alias]);
    expect(runtimeModesFor("claude", "claude:opus-5-5")).toContain("auto");
    expect(coerceRuntimeMode("claude", "claude:opus-5-5", "auto")).toBe("auto");
  });

  it.each([false, undefined])(
    "prefers exact support %s over an alias target",
    (supportsAuto) => {
      setHarnessModels("claude", [
        alias,
        {
          id: "claude:opus-5-5",
          harness: "claude",
          name: "Opus 5.5",
          nativeId: "claude-opus-5-5",
          supportsAuto,
        },
      ]);
      expect(runtimeModesFor("claude", "claude:opus-5-5")).not.toContain(
        "auto",
      );
      expect(coerceRuntimeMode("claude", "claude:opus-5-5", "auto")).toBe(
        "auto-accept-edits",
      );
    },
  );

  it("does not borrow alias support from another version", () => {
    setHarnessModels("claude", [alias]);
    expect(runtimeModesFor("claude", "claude:opus-5")).not.toContain("auto");
    expect(coerceRuntimeMode("claude", "claude:opus-5", "auto")).toBe(
      "auto-accept-edits",
    );
  });

  it("uses the supplied catalog and keeps alias matches within the harness", () => {
    const catalog = (entries: AgentModel[]): ModelCatalog => ({
      find: (id) => entries.find((model) => model.id === id),
      modelsFor: () => entries,
    });
    expect(
      coerceRuntimeMode("claude", "claude:opus-5-5", "auto", catalog([alias])),
    ).toBe("auto");
    expect(
      coerceRuntimeMode(
        "claude",
        "claude:opus-5-5",
        "auto",
        catalog([{ ...alias, id: "codex:opus", harness: "codex" }]),
      ),
    ).toBe("auto-accept-edits");
  });

  const discover = (id: string, supportsAuto: boolean) =>
    setHarnessModels("claude", [
      {
        id,
        harness: "claude",
        name: "Opus 4.7",
        nativeId: "claude-opus-4-7",
        supportsAuto,
      },
    ]);

  it("picks up confirmed Auto once discovery arrives", () => {
    // Saved before discovery, with the picker's dotted id.
    expect(runtimeModesFor("claude", "claude:opus-4.7")).not.toContain("auto");
    expect(coerceRuntimeMode("claude", "claude:opus-4.7", "auto")).toBe(
      "auto-accept-edits",
    );
    discover("claude:opus-4-7", true);
    expect(runtimeModesFor("claude", "claude:opus-4.7")).toContain("auto");
    expect(coerceRuntimeMode("claude", "claude:opus-4.7", "auto")).toBe("auto");
  });

  it("follows a discovered entry over a bundled one with the saved id", () => {
    setHarnessModels("claude", [
      {
        id: "claude:opus-4-6",
        harness: "claude",
        name: "Opus 4.6",
        nativeId: "claude-opus-4-6",
        supportsAuto: true,
      },
    ]);
    expect(runtimeModesFor("claude", "claude:opus-4.6")).toContain("auto");
  });

  it("prefers the exact picker id", () => {
    setHarnessModels("claude", [
      {
        id: "claude:opus-4.7",
        harness: "claude",
        name: "Opus 4.7",
        nativeId: "claude-opus-4-7",
        supportsAuto: false,
      },
      {
        id: "claude:opus-4-7",
        harness: "claude",
        name: "Opus 4.7",
        nativeId: "claude-opus-4-7",
        supportsAuto: true,
      },
    ]);
    expect(runtimeModesFor("claude", "claude:opus-4.7")).not.toContain("auto");
  });

  it("does not borrow support from another version", () => {
    discover("claude:opus-4-7", true);
    expect(runtimeModesFor("claude", "claude:opus-4.6")).not.toContain("auto");
  });
});
