import { afterEach, describe, expect, it, vi } from "vitest";
vi.mock("../../../integrations/harness/core/availability", () => ({
  isHarnessAvailable: vi.fn((id: string) => id === "codex" || id === "claude"),
  probeHarnessAvailability: vi.fn(async () => {}),
}));
vi.mock("../../../integrations/harness/core/registry", () => ({
  refreshHarnessCatalogs: vi.fn(async () => {}),
}));
import { discoverOrchestrationSettings } from "./orchestrationCatalog";
import {
  isHarnessAvailable,
  probeHarnessAvailability,
} from "../../../integrations/harness/core/availability";
import { refreshHarnessCatalogs } from "../../../integrations/harness/core/registry";
import {
  resetHarnessModelOverlays,
  savePickerProviderVisible,
  setHarnessModels,
} from "../../sessions/model/models";
import { setProjectProviderHidden } from "../../sessions/model/projectProviders";

afterEach(() => {
  vi.unstubAllGlobals();
  resetHarnessModelOverlays();
  vi.clearAllMocks();
  vi.mocked(isHarnessAvailable).mockImplementation(
    (id) => id === "codex" || id === "claude",
  );
});

describe("automatic orchestration catalog", () => {
  it("discovers every installed harness and reads its refreshed models without a user-selected pool", async () => {
    vi.mocked(refreshHarnessCatalogs).mockImplementationOnce(async () => {
      setHarnessModels("codex", [
        { id: "codex:live", harness: "codex", name: "Live Codex" },
      ]);
    });
    const settings = await discoverOrchestrationSettings();
    expect(probeHarnessAvailability).toHaveBeenCalledOnce();
    expect(refreshHarnessCatalogs).toHaveBeenCalledWith(["claude", "codex"]);
    expect(settings.choices).toContainEqual({
      harness: "codex",
      model: "codex:live",
      name: "Live Codex",
    });
    expect(settings.choices.some((choice) => choice.harness === "claude")).toBe(
      true,
    );
    expect(settings.choices.some((choice) => choice.harness === "cursor")).toBe(
      false,
    );
    expect(settings.maxWorkers).toBe(2);
  });
  it("does not truncate catalogs at the former manual-selection limit", async () => {
    setHarnessModels(
      "codex",
      Array.from({ length: 80 }, (_, i) => ({
        id: `codex:${i}`,
        harness: "codex",
        name: `Model ${i}`,
      })),
    );
    const settings = await discoverOrchestrationSettings();
    expect(
      settings.choices.filter((choice) => choice.harness === "codex"),
    ).toHaveLength(80);
  });
  it("leaves providers hidden from the picker out of discovery", async () => {
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
    vi.mocked(isHarnessAvailable).mockImplementation(
      (id) => id === "codex" || id === "cursor",
    );
    savePickerProviderVisible("cursor", false);
    setHarnessModels("codex", [
      { id: "codex:live", harness: "codex", name: "Live Codex" },
    ]);
    const settings = await discoverOrchestrationSettings();
    expect(refreshHarnessCatalogs).toHaveBeenCalledWith(["codex"]);
    expect(settings.choices.some((choice) => choice.harness === "cursor")).toBe(
      false,
    );
  });
  it("excludes providers hidden in this project but keeps them in others", async () => {
    const data = new Map<string, string>();
    vi.stubGlobal("localStorage", {
      getItem: (key: string) => data.get(key) ?? null,
      setItem: (key: string, value: string) => {
        data.set(key, value);
      },
    });
    vi.mocked(isHarnessAvailable).mockImplementation(
      (id) => id === "codex" || id === "cursor",
    );
    setHarnessModels("codex", [
      { id: "codex:live", harness: "codex", name: "Live Codex" },
    ]);
    setHarnessModels("cursor", [
      { id: "cursor:live", harness: "cursor", name: "Live Cursor" },
    ]);
    setProjectProviderHidden("/repo", "cursor", true);

    const hidden = await discoverOrchestrationSettings("/repo");
    expect(refreshHarnessCatalogs).toHaveBeenCalledWith(["codex"]);
    expect(hidden.choices.some((choice) => choice.harness === "cursor")).toBe(
      false,
    );

    vi.mocked(refreshHarnessCatalogs).mockClear();
    const visible = await discoverOrchestrationSettings("/other");
    expect(refreshHarnessCatalogs).toHaveBeenCalledWith(["codex", "cursor"]);
    expect(visible.choices.some((choice) => choice.harness === "cursor")).toBe(
      true,
    );
  });
  it("fails planning clearly when no harness is available", async () => {
    vi.mocked(isHarnessAvailable).mockReturnValue(false);
    await expect(discoverOrchestrationSettings()).rejects.toThrow(
      "No worker models are available",
    );
  });
});
