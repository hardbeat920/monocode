import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  clearProjectProviders,
  loadProjectProviderSettings,
  projectProvidersRevision,
  rebaseProjectProviders,
  setProjectDefaultModel,
  setProjectDefaultProvider,
  setProjectProviderHidden,
} from "./projectProviders";

describe("project provider settings", () => {
  let storage: Map<string, string>;

  beforeEach(() => {
    storage = new Map();
    vi.stubGlobal("localStorage", {
      getItem: (key: string) => storage.get(key) ?? null,
      setItem: (key: string, value: string) => {
        storage.set(key, value);
      },
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("is empty by default and falls back to global", () => {
    expect(loadProjectProviderSettings("/repo/a")).toEqual({});
    expect(loadProjectProviderSettings(undefined)).toEqual({});
  });

  it("stores a project default provider and model", () => {
    setProjectDefaultProvider("/repo/a", "claude", "claude:opus-5");
    expect(loadProjectProviderSettings("/repo/a")).toEqual({
      defaultHarness: "claude",
      defaultModel: "claude:opus-5",
    });
  });

  it("stores per-provider models and hidden providers", () => {
    setProjectDefaultModel("/repo/a", "cursor", "cursor:composer-2.5");
    setProjectProviderHidden("/repo/a", "grok", true);
    setProjectProviderHidden("/repo/a", "omp", true);
    setProjectProviderHidden("/repo/a", "grok", false);
    expect(loadProjectProviderSettings("/repo/a")).toEqual({
      models: { cursor: "cursor:composer-2.5" },
      hidden: ["omp"],
    });
  });

  it("scopes settings to the project path", () => {
    setProjectProviderHidden("/repo/a", "cursor", true);
    expect(loadProjectProviderSettings("/repo/b")).toEqual({});
  });

  it("treats trailing slashes as the same project", () => {
    setProjectProviderHidden("/repo/a/", "cursor", true);
    expect(loadProjectProviderSettings("/repo/a").hidden).toEqual(["cursor"]);
  });

  it("drops the project entry once nothing is overridden", () => {
    setProjectProviderHidden("/repo/a", "cursor", true);
    setProjectProviderHidden("/repo/a", "cursor", false);
    expect(storage.get("monocode.projectProviderSettings.v1")).toBe("{}");
  });

  it("clears and rebases project entries", () => {
    setProjectProviderHidden("/repo/a", "cursor", true);
    clearProjectProviders("/repo/a");
    expect(loadProjectProviderSettings("/repo/a")).toEqual({});

    setProjectProviderHidden("/repo/a", "codex", true);
    rebaseProjectProviders("/repo/a", "/repo/renamed");
    expect(loadProjectProviderSettings("/repo/a")).toEqual({});
    expect(loadProjectProviderSettings("/repo/renamed").hidden).toEqual([
      "codex",
    ]);
  });

  it("ignores malformed storage", () => {
    storage.set("monocode.projectProviderSettings.v1", "{not json");
    expect(loadProjectProviderSettings("/repo/a")).toEqual({});
    storage.set("monocode.projectProviderSettings.v1", JSON.stringify([1, 2]));
    expect(loadProjectProviderSettings("/repo/a")).toEqual({});
  });

  it("bumps the revision on writes", () => {
    const before = projectProvidersRevision();
    setProjectProviderHidden("/repo/a", "cursor", true);
    expect(projectProvidersRevision()).toBeGreaterThan(before);
  });
});
