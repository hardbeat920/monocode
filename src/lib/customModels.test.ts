// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  customModelId,
  MAX_CUSTOM_MODEL_COUNT,
  MAX_CUSTOM_MODEL_LENGTH,
  readCustomModelEntries,
  type CustomModelHarness,
} from "./customModels";
import { CUSTOM_MODEL_PRESETS } from "./customModelEditor";
import {
  addCustomModel,
  allModels,
  defaultSessionChoice,
  findModel,
  getModelSnapshot,
  hasLiveCatalog,
  loadCustomModels,
  loadDefaultModels,
  loadFavoriteModels,
  modelsFor,
  nativeModelId,
  preferredModelSettings,
  removeCustomModel,
  resetHarnessModelOverlays,
  resolveModel,
  saveFavoriteModels,
  saveLastModelChoice,
  setHarnessModels,
  subscribeModels,
  updateCustomModel,
  type AgentModel,
} from "./models";

const codex: AgentModel = {
  id: "codex:gpt-catalog",
  harness: "codex",
  nativeId: "gpt-catalog",
  name: "Catalog model",
  settings: CUSTOM_MODEL_PRESETS.codex,
};

beforeEach(() => {
  const data = new Map<string, string>();
  vi.stubGlobal("localStorage", {
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => {
      data.set(key, value);
    },
    removeItem: (key: string) => {
      data.delete(key);
    },
    clear: () => data.clear(),
  });
  resetHarnessModelOverlays();
});

afterEach(() => {
  vi.restoreAllMocks();
  localStorage.clear();
  resetHarnessModelOverlays();
  vi.unstubAllGlobals();
});

describe("custom model catalogs", () => {
  it.each<CustomModelHarness>(["claude", "codex"])(
    "preserves opaque %s IDs through persistence and resolution",
    (harness) => {
      const slug = "Vendor/Model:Preview[1m]";
      const id = customModelId(harness, slug);
      expect(addCustomModel(harness, `  ${slug}  `)).toBeNull();
      saveLastModelChoice(harness, id);
      resetHarnessModelOverlays();

      expect(loadCustomModels(harness)).toEqual([
        { slug, name: slug, settings: null },
      ]);
      expect(resolveModel(harness, defaultSessionChoice().model)).toMatchObject(
        { id, harness, nativeId: slug, isCustom: true },
      );
      expect(nativeModelId(id)).toBe(slug);
      expect(allModels().filter((model) => model.id === id)).toHaveLength(1);
    },
  );

  it("allows custom Claude slugs that collide with MonoCode's shortened built-in IDs", () => {
    expect(addCustomModel("claude", "sonnet-5")).toBeNull();
    expect(nativeModelId("claude:sonnet-5")).toBe("claude-sonnet-5");
    expect(nativeModelId(customModelId("claude", "sonnet-5"))).toBe("sonnet-5");
  });

  it("keeps providers separate and survives catalog refreshes", () => {
    addCustomModel("claude", "private-model");
    addCustomModel("codex", "private-model");
    expect(hasLiveCatalog("codex")).toBe(false);
    setHarnessModels("codex", [codex]);
    expect(hasLiveCatalog("codex")).toBe(true);
    expect(modelsFor("codex").map((model) => model.nativeId)).toEqual([
      "gpt-catalog",
      "private-model",
    ]);
    expect(loadCustomModels("claude")).toHaveLength(1);
    expect(
      preferredModelSettings(
        resolveModel("codex", customModelId("codex", "private-model")),
      ),
    ).toEqual({ reasoningEffort: "medium", serviceTier: "default" });
    expect(
      preferredModelSettings(
        resolveModel("claude", customModelId("claude", "private-model")),
      ),
    ).toEqual({});
  });

  it("lets an exact CLI model take precedence without resurrecting removed custom entries", () => {
    addCustomModel("codex", "gpt-catalog");
    setHarnessModels("codex", [codex]);
    expect(modelsFor("codex")).toEqual([codex]);
    expect(resolveModel("codex", customModelId("codex", "gpt-catalog"))).toBe(
      codex,
    );
    removeCustomModel("codex", "gpt-catalog");
    setHarnessModels("codex", [{ ...codex, id: "codex:new", nativeId: "new" }]);
    expect(modelsFor("codex").map((model) => model.nativeId)).toEqual(["new"]);
  });

  it("preserves custom names and options over Codex defaults after a reload", () => {
    addCustomModel("codex", "private-model");
    addCustomModel("claude", "other-model");
    const settings = [{ ...CUSTOM_MODEL_PRESETS.codex[0], value: "xhigh" }];
    expect(
      updateCustomModel("codex", {
        slug: "private-model",
        name: "My reasoning model",
        settings,
      }),
    ).toBeNull();
    resetHarnessModelOverlays();
    setHarnessModels("codex", [codex]);
    const model = resolveModel(
      "codex",
      customModelId("codex", "private-model"),
    );
    expect(model.name).toBe("My reasoning model");
    expect(preferredModelSettings(model)).toEqual({ reasoningEffort: "xhigh" });
    expect(loadCustomModels("claude")).toHaveLength(1);
  });

  it("invalidates cached catalogs and notifies subscribers on local and cross-window edits", () => {
    const before = modelsFor("codex");
    expect(modelsFor("codex")).toBe(before);
    const changed = vi.fn();
    const unsubscribe = subscribeModels(changed);
    try {
      addCustomModel("codex", "first");
      expect(changed).toHaveBeenCalledTimes(1);
      expect(modelsFor("codex")).not.toBe(before);
      const current = modelsFor("codex");
      expect(modelsFor("codex")).toBe(current);

      localStorage.setItem(
        "monocode.customModels",
        JSON.stringify({ codex: ["second"] }),
      );
      window.dispatchEvent(
        new StorageEvent("storage", { key: "monocode.customModels" }),
      );
      expect(modelsFor("codex").map((model) => model.nativeId)).toEqual([
        "second",
      ]);
      expect(findModel(customModelId("codex", "first"))).toBeUndefined();
      expect(changed).toHaveBeenCalledTimes(2);

      localStorage.clear();
      window.dispatchEvent(new StorageEvent("storage", { key: null }));
      expect(modelsFor("codex")).toEqual([]);
    } finally {
      unsubscribe();
    }
  });

  it("removes defaults and favorites without changing the model of an open session", () => {
    setHarnessModels("codex", [codex]);
    const slug = "gpt-catalog:private[preview]";
    const id = customModelId("codex", slug);
    addCustomModel("codex", slug);
    saveLastModelChoice("codex", id);
    saveFavoriteModels([id, codex.id]);

    expect(removeCustomModel("codex", slug)).toBeNull();
    expect(loadDefaultModels().codex).toBe(codex.id);
    expect(defaultSessionChoice()).toEqual({
      harness: "codex",
      model: codex.id,
    });
    expect(loadFavoriteModels()).toEqual([codex.id]);
    expect(findModel(id)).toBeUndefined();
    expect(resolveModel("codex", id)).toMatchObject({
      id,
      harness: "codex",
      nativeId: slug,
    });
    expect(nativeModelId(id)).toBe(slug);
    expect(
      updateCustomModel("codex", { slug, name: slug, settings: null }),
    ).toContain("removed");
  });
});

describe("custom model validation", () => {
  it("rejects empty, duplicate, discovered and overlong IDs", () => {
    setHarnessModels("codex", [codex]);
    expect(addCustomModel("codex", " \n ")).toContain("Enter a model ID");
    expect(addCustomModel("codex", "gpt-catalog")).toContain(
      "already provided",
    );
    expect(addCustomModel("codex", "private")).toBeNull();
    expect(addCustomModel("codex", " private ")).toContain("already saved");
    expect(
      addCustomModel("codex", "x".repeat(MAX_CUSTOM_MODEL_LENGTH + 1)),
    ).toContain("256");
    expect(
      addCustomModel("codex", "x".repeat(MAX_CUSTOM_MODEL_LENGTH)),
    ).toBeNull();
    expect(loadCustomModels("codex")).toHaveLength(2);
  });

  it("limits each provider to 32 custom models", () => {
    for (let index = 0; index < MAX_CUSTOM_MODEL_COUNT; index++) {
      expect(addCustomModel("codex", `model-${index}`)).toBeNull();
    }
    expect(addCustomModel("codex", "overflow")).toContain("32");
    expect(addCustomModel("claude", "overflow")).toBeNull();
    removeCustomModel("codex", "model-0");
    expect(addCustomModel("codex", "replacement")).toBeNull();
  });

  it("tolerates malformed storage and drops invalid option definitions", () => {
    expect(
      readCustomModelEntries([
        null,
        10,
        [],
        " ",
        { slug: false },
        " first ",
        { slug: "first", name: "Duplicate" },
        { slug: "second", name: " Friendly name ", settings: "invalid" },
        { slug: "third", settings: [{ id: "effort", kind: "select" }] },
        "x".repeat(MAX_CUSTOM_MODEL_LENGTH + 1),
      ]),
    ).toEqual([
      { slug: "first", name: "first", settings: null },
      { slug: "second", name: "Friendly name", settings: null },
      { slug: "third", name: "third", settings: null },
    ]);
    localStorage.setItem("monocode.customModels", "not JSON");
    expect(loadCustomModels("codex")).toEqual([]);
    expect(addCustomModel("codex", "recovered")).toBeNull();
  });

  it("reports a failed write without publishing unsaved models", () => {
    addCustomModel("codex", "saved");
    const before = getModelSnapshot();
    vi.spyOn(localStorage, "setItem").mockImplementation(() => {
      throw new Error("Quota exceeded");
    });
    expect(addCustomModel("codex", "unsaved")).toContain("Could not save");
    expect(removeCustomModel("codex", "saved")).toContain("Could not save");
    expect(loadCustomModels("codex").map((entry) => entry.slug)).toEqual([
      "saved",
    ]);
    expect(getModelSnapshot()).toBe(before);
  });
});
