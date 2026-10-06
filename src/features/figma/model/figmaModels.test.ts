// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { defaultSessionChoice } from "../../sessions/model/models";
import {
  FIGMA_MODELS_CHANGE_EVENT,
  clearFigmaModelPick,
  figmaModelsRevision,
  loadFigmaDefaultModel,
  pickFigmaModel,
  resolveFigmaModel,
  saveFigmaDefaultModel,
} from "./figmaModels";

const APP = "/work/app";
const SITE = "/work/site";
let storage: Map<string, string>;

beforeEach(() => {
  storage = new Map<string, string>();
  vi.stubGlobal("localStorage", {
    getItem: (key: string) => storage.get(key) ?? null,
    setItem: (key: string, value: string) => storage.set(key, value),
    removeItem: (key: string) => storage.delete(key),
  });
});

afterEach(() => {
  clearFigmaModelPick(APP);
  clearFigmaModelPick(SITE);
  vi.unstubAllGlobals();
});

describe("figma generation model", () => {
  it("starts with the project's new-session agent and model", () => {
    const project = defaultSessionChoice(APP);
    expect(resolveFigmaModel(APP)).toEqual({
      choice: {
        harness: project.harness,
        model: project.model,
        modelSettings: {},
      },
      source: "project",
    });
  });

  it("uses the Figma default over the project default", () => {
    saveFigmaDefaultModel({
      harness: "codex",
      model: "gpt-5",
      modelSettings: { effort: "high" },
    });
    expect(resolveFigmaModel(APP)).toEqual({
      choice: {
        harness: "codex",
        model: "gpt-5",
        modelSettings: { effort: "high" },
      },
      source: "figma",
    });
  });

  it("keeps a panel pick for its own project only", () => {
    saveFigmaDefaultModel({
      harness: "codex",
      model: "gpt-5",
      modelSettings: {},
    });
    pickFigmaModel(APP, {
      harness: "claude",
      model: "opus",
      modelSettings: {},
    });
    expect(resolveFigmaModel(APP)).toMatchObject({
      choice: { harness: "claude", model: "opus" },
      source: "picked",
    });
    expect(resolveFigmaModel(SITE)).toMatchObject({
      choice: { harness: "codex", model: "gpt-5" },
      source: "figma",
    });
    clearFigmaModelPick(APP);
    expect(resolveFigmaModel(APP).source).toBe("figma");
  });

  it("persists the default and clears it back to the project default", () => {
    saveFigmaDefaultModel({
      harness: "codex",
      model: "gpt-5",
      modelSettings: { effort: "high" },
    });
    expect(
      JSON.parse(storage.get("monocode.figmaGenerationModel.v1") ?? ""),
    ).toEqual({
      harness: "codex",
      model: "gpt-5",
      modelSettings: { effort: "high" },
    });
    saveFigmaDefaultModel(null);
    expect(storage.has("monocode.figmaGenerationModel.v1")).toBe(false);
    expect(loadFigmaDefaultModel()).toBeNull();
    expect(resolveFigmaModel(APP).source).toBe("project");
  });

  it("ignores a stored default it cannot trust", () => {
    storage.set("monocode.figmaGenerationModel.v1", "{not json");
    expect(loadFigmaDefaultModel()).toBeNull();
    storage.set(
      "monocode.figmaGenerationModel.v1",
      JSON.stringify({ harness: "unknown", model: "x" }),
    );
    expect(loadFigmaDefaultModel()).toBeNull();
    storage.set(
      "monocode.figmaGenerationModel.v1",
      JSON.stringify({
        harness: "claude",
        model: "opus",
        modelSettings: { effort: "high", broken: 3 },
      }),
    );
    expect(loadFigmaDefaultModel()).toEqual({
      harness: "claude",
      model: "opus",
      modelSettings: { effort: "high" },
    });
  });

  it("announces every change so open views re-render", () => {
    const listener = vi.fn();
    window.addEventListener(FIGMA_MODELS_CHANGE_EVENT, listener);
    const before = figmaModelsRevision();
    pickFigmaModel(APP, {
      harness: "claude",
      model: "opus",
      modelSettings: {},
    });
    saveFigmaDefaultModel(null);
    clearFigmaModelPick(APP);
    clearFigmaModelPick(APP);
    window.removeEventListener(FIGMA_MODELS_CHANGE_EVENT, listener);
    expect(listener).toHaveBeenCalledTimes(3);
    expect(figmaModelsRevision()).toBe(before + 3);
  });
});
