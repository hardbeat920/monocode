import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { HarnessId } from "./session";
import {
  coerceModelPickerTab,
  defaultModelId,
  defaultSessionChoice,
  hasLiveCatalog,
  isPickerProviderVisible,
  loadDefaultModels,
  loadHiddenPickerProviders,
  loadLastModelChoice,
  loadLastModelSettings,
  mergeModelSettings,
  modelPickerTabs,
  preferredModelId,
  preferredModelSettings,
  resetHarnessModelOverlays,
  saveDefaultModel,
  saveLastModelChoice,
  saveLastModelSettings,
  savePickerProviderVisible,
  setHarnessModels,
  showProviderInModelPicker,
  stepModelPickerTab,
  type AgentModel,
} from "./models";

const opus: AgentModel = {
  id: "claude:opus-5",
  harness: "claude",
  name: "Opus 5",
  settings: [
    {
      id: "effort",
      label: "Reasoning",
      kind: "select",
      value: "high",
      options: [
        { value: "high", label: "High" },
        { value: "xhigh", label: "Extra High" },
        { value: "max", label: "Max" },
      ],
    },
    {
      id: "fast",
      label: "Fast",
      kind: "toggle",
      value: "false",
      options: [
        { value: "true", label: "On" },
        { value: "false", label: "Off" },
      ],
    },
  ],
};

const haiku: AgentModel = {
  id: "claude:haiku-4.5",
  harness: "claude",
  name: "Haiku 4.5",
  settings: [
    {
      id: "thinking",
      label: "Thinking",
      kind: "toggle",
      value: "false",
      options: [
        { value: "true", label: "On" },
        { value: "false", label: "Off" },
      ],
    },
  ],
};

function mockLocalStorage() {
  const data = new Map<string, string>();
  const storage = {
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => {
      data.set(key, value);
    },
    removeItem: (key: string) => {
      data.delete(key);
    },
    clear: () => {
      data.clear();
    },
    key: (index: number) => [...data.keys()][index] ?? null,
    get length() {
      return data.size;
    },
  };
  Object.defineProperty(globalThis, "localStorage", {
    value: storage,
    configurable: true,
  });
}

describe("model settings memory", () => {
  beforeEach(() => {
    mockLocalStorage();
  });

  afterEach(() => {
    mockLocalStorage();
  });

  it("keeps valid current values when merging onto a model", () => {
    expect(
      mergeModelSettings(opus, { effort: "xhigh", fast: "true" }),
    ).toEqual({ effort: "xhigh", fast: "true" });
  });

  it("drops values the new model does not support", () => {
    expect(
      mergeModelSettings(haiku, { effort: "xhigh", fast: "true" }),
    ).toEqual({ thinking: "false" });
  });

  it("maps extra-high onto Claude's xhigh", () => {
    expect(mergeModelSettings(opus, { effort: "extra-high" })).toEqual({
      effort: "xhigh",
      fast: "false",
    });
  });

  it("remembers extra-high and fast across models that support them", () => {
    saveLastModelSettings({ effort: "xhigh", fast: "true" });
    expect(preferredModelSettings(opus)).toEqual({
      effort: "xhigh",
      fast: "true",
    });
    expect(preferredModelSettings(haiku)).toEqual({ thinking: "false" });
  });

  it("merges newly saved settings into previously stored ones", () => {
    saveLastModelSettings({ effort: "xhigh", fast: "true" });
    saveLastModelSettings({ thinking: "true" });
    expect(loadLastModelSettings()).toEqual({
      effort: "xhigh",
      fast: "true",
      thinking: "true",
    });
  });

  it("applies stored preferences over a session's current values", () => {
    saveLastModelSettings({ effort: "xhigh", fast: "true" });
    expect(preferredModelSettings(opus, { effort: "high", fast: "false" })).toEqual({
      effort: "xhigh",
      fast: "true",
    });
  });

  it("fill mode keeps stored preferences when the session still has defaults", () => {
    saveLastModelSettings({ effort: "xhigh", fast: "true" });
    saveLastModelSettings({ effort: "high", fast: "false" }, "fill");
    expect(loadLastModelSettings()).toEqual({
      effort: "xhigh",
      fast: "true",
    });
  });

  it("fill mode records session values that have not been stored yet", () => {
    saveLastModelSettings({ effort: "xhigh" }, "fill");
    expect(loadLastModelSettings()).toEqual({ effort: "xhigh" });
  });

  it("uses the current session when nothing has been stored yet", () => {
    expect(
      preferredModelSettings(opus, { effort: "xhigh", fast: "true" }),
    ).toEqual({ effort: "xhigh", fast: "true" });
  });
});

describe("provider defaults", () => {
  beforeEach(() => {
    mockLocalStorage();
  });

  afterEach(() => {
    mockLocalStorage();
  });

  it("remembers a model per provider without changing the default provider", () => {
    saveLastModelChoice("cursor", "cursor:grok-4.6");
    saveDefaultModel("claude", "claude:opus-5");
    saveDefaultModel("opencode", "opencode:glm-5");
    expect(loadLastModelChoice()).toEqual({
      harness: "cursor",
      model: "cursor:grok-4.6",
    });
    expect(loadDefaultModels()).toEqual({
      cursor: "cursor:grok-4.6",
      claude: "claude:opus-5",
      opencode: "opencode:glm-5",
    });
    expect(preferredModelId("claude")).toBe("claude:opus-5");
    expect(preferredModelId("cursor")).toBe("cursor:grok-4.6");
  });

  it("falls back to lastModel for the default provider when no map exists", () => {
    localStorage.setItem(
      "monocode.lastModel",
      JSON.stringify({ harness: "cursor", model: "cursor:grok-4.6" }),
    );
    expect(preferredModelId("cursor")).toBe("cursor:grok-4.6");
    expect(preferredModelId("claude")).toBe(defaultModelId("claude"));
  });

  it("uses the saved default provider and its model for new sessions", () => {
    saveLastModelChoice("claude", "claude:opus-5");
    expect(defaultSessionChoice()).toEqual({
      harness: "claude",
      model: "claude:opus-5",
    });
  });

  it("keeps catalog defaults when nothing is saved", () => {
    expect(defaultSessionChoice()).toEqual({
      harness: "cursor",
      model: defaultModelId("cursor"),
    });
  });
});

describe("model picker tabs", () => {
  const available = (id: HarnessId) =>
    id === "claude" || id === "fx" || id === "cursor";

  it("starts with favorites then installed providers", () => {
    expect(modelPickerTabs(available)).toEqual([
      "favorites",
      "claude",
      "cursor",
      "fx",
    ]);
  });

  it("wraps left and right across favorites and providers", () => {
    expect(stepModelPickerTab("favorites", 1, available)).toBe("claude");
    expect(stepModelPickerTab("claude", 1, available)).toBe("cursor");
    expect(stepModelPickerTab("fx", 1, available)).toBe("favorites");
    expect(stepModelPickerTab("favorites", -1, available)).toBe("fx");
  });

  it("treats an unavailable current tab as the start of the list", () => {
    expect(stepModelPickerTab("pi", 1, available)).toBe("claude");
  });

  it("falls back to favorites when the current tab is hidden", () => {
    expect(coerceModelPickerTab("pi", available)).toBe("favorites");
    expect(coerceModelPickerTab("cursor", available)).toBe("cursor");
    expect(coerceModelPickerTab("favorites", available)).toBe("favorites");
  });
});

describe("picker provider visibility", () => {
  beforeEach(mockLocalStorage);
  afterEach(mockLocalStorage);

  it("shows every provider until the user hides one", () => {
    expect(loadHiddenPickerProviders()).toEqual([]);
    expect(isPickerProviderVisible("pi")).toBe(true);
    savePickerProviderVisible("pi", false);
    savePickerProviderVisible("omp", false);
    expect(isPickerProviderVisible("pi")).toBe(false);
    expect(isPickerProviderVisible("omp")).toBe(false);
    expect(isPickerProviderVisible("claude")).toBe(true);
    expect(loadHiddenPickerProviders()).toEqual(["pi", "omp"]);
    savePickerProviderVisible("pi", true);
    expect(isPickerProviderVisible("pi")).toBe(true);
    expect(loadHiddenPickerProviders()).toEqual(["omp"]);
  });

  it("omits hidden providers even before an install probe", () => {
    savePickerProviderVisible("fx", false);
    expect(showProviderInModelPicker("fx", true, false)).toBe(false);
    expect(showProviderInModelPicker("claude", true, false)).toBe(true);
  });

  it("omits uninstalled providers after the probe, keeps them before", () => {
    expect(showProviderInModelPicker("pi", false, false)).toBe(true);
    expect(showProviderInModelPicker("pi", false, true)).toBe(false);
    expect(showProviderInModelPicker("pi", true, true)).toBe(true);
  });
});

describe("live catalog overlays", () => {
  afterEach(() => {
    resetHarnessModelOverlays();
  });

  it("is empty until a CLI catalog replaces the fallback list", () => {
    expect(hasLiveCatalog("pi")).toBe(false);
    setHarnessModels("pi", [
      {
        id: "pi:opus",
        harness: "pi",
        name: "Opus",
        nativeId: "anthropic/opus",
      },
    ]);
    expect(hasLiveCatalog("pi")).toBe(true);
    expect(hasLiveCatalog("omp")).toBe(false);
  });
});
