// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  registerHarness,
  type HarnessAdapter,
} from "../../../integrations/harness/core/registry";
import {
  resetHarnessModelOverlays,
  setHarnessModels,
} from "../../sessions/model/models";
import { recordUsedModel } from "../../sessions/model/usedModels";
import { appCapabilities } from "./capabilities";

vi.mock("../../../integrations/harness/core/availability", () => ({
  isHarnessAvailable: (id: string) => id === "pi" || id === "codex",
}));

function adapter(
  id: "pi" | "codex",
  refreshCatalog: HarnessAdapter["refreshCatalog"],
): HarnessAdapter {
  return {
    id,
    live: true,
    refreshCatalog,
    sendTurn: vi.fn(),
    steerTurn: vi.fn(),
    cancelTurn: vi.fn(),
    respondApproval: vi.fn(),
    stopSession: vi.fn(),
    forgetSession: vi.fn(),
    bindSession: vi.fn(),
  };
}

const piRefresh = vi.fn(async () => ({ status: "succeeded" as const }));
const codexRefresh = vi.fn(async () => ({ status: "succeeded" as const }));
beforeEach(() => {
  localStorage.clear();
  piRefresh.mockReset().mockResolvedValue({ status: "succeeded" });
  codexRefresh.mockReset().mockResolvedValue({ status: "succeeded" });
  registerHarness(adapter("pi", piRefresh));
  registerHarness(adapter("codex", codexRefresh));
  setHarnessModels(
    "pi",
    Array.from({ length: 35 }, (_, index) => ({
      harness: "pi",
      id: `pi:provider/model-${index}`,
      nativeId: `provider/model-${index}`,
      name: `Requested Model ${index}`,
      provider: { id: "provider", name: "Upstream" },
    })),
  );
  setHarnessModels("codex", [
    { id: "codex:other", harness: "codex", name: "Other" },
  ]);
});
afterEach(resetHarnessModelOverlays);

describe("compact app capabilities", () => {
  it("returns counts/status and actual-use recents, never catalogs or eager probes by default", async () => {
    recordUsedModel({
      observationId: "turn",
      harness: "pi",
      model: "pi:provider/model-0",
      identitySource: "requested",
      usedAt: 1,
    });
    const result = await appCapabilities({});
    expect(result.harnesses.find((h) => h.id === "pi")).toEqual({
      id: "pi",
      available: true,
      count: 35,
      catalog: { source: "live", refresh: { status: "not-requested" } },
    });
    expect(result.harnesses.find((h) => h.id === "claude")).toMatchObject({
      available: false,
      catalog: { source: "fallback" },
    });
    expect(result.harnesses.every((h) => !("models" in h))).toBe(true);
    expect(result.recentModels).toMatchObject([
      { model: "pi:provider/model-0", identitySource: "requested" },
    ]);
    expect(result.runtimeModes.length).toBeGreaterThan(0);
    expect(piRefresh).not.toHaveBeenCalled();
    expect(codexRefresh).not.toHaveBeenCalled();
  });
  it("refreshes only the requested harness and enumerates its complete catalog", async () => {
    const result = await appCapabilities({ harness: "pi" });
    expect(result.harnesses).toHaveLength(1);
    expect(result.harnesses[0].models).toHaveLength(35);
    expect(result.harnesses[0].catalog.refresh.status).toBe("succeeded");
    expect(piRefresh).toHaveBeenCalledOnce();
    expect(codexRefresh).not.toHaveBeenCalled();
  });
  it.each(["requested model", "PROVIDER/MODEL", "upstream", "PI:PROVIDER"])(
    "returns all partial scoped matches for %s with exact IDs",
    async (model) => {
      const result = await appCapabilities({ harness: "pi", model });
      expect(result.harnesses[0].models).toHaveLength(35);
      expect(result.harnesses[0].models?.[34]).toMatchObject({
        id: "pi:provider/model-34",
        nativeId: "provider/model-34",
        provider: { id: "provider", name: "Upstream" },
        settings: [],
      });
      expect(codexRefresh).not.toHaveBeenCalled();
    },
  );
  it("searches only available harnesses' loaded catalogs without any probes", async () => {
    setHarnessModels("claude", [
      { id: "claude:absent", harness: "claude", name: "Requested Model" },
    ]);
    const result = await appCapabilities({ model: "Requested" });
    expect(result.harnesses.map((h) => h.id)).toEqual(["codex", "pi"]);
    expect(result.harnesses.find((h) => h.id === "pi")?.models).toHaveLength(
      35,
    );
    expect(result.harnesses.find((h) => h.id === "codex")?.models).toEqual([]);
    expect(piRefresh).not.toHaveBeenCalled();
    expect(codexRefresh).not.toHaveBeenCalled();
  });
  it("describes an explicit unavailable harness without probing", async () => {
    const result = await appCapabilities({ harness: "claude" });
    expect(result.harnesses[0]).toMatchObject({
      id: "claude",
      available: false,
      catalog: { source: "fallback" },
    });
    expect(result.harnesses[0].models?.length).toBeGreaterThan(0);
    expect(piRefresh).not.toHaveBeenCalled();
    expect(codexRefresh).not.toHaveBeenCalled();
  });
  it("keeps failed refresh explicit over a retained live overlay, including the next overview", async () => {
    piRefresh.mockRejectedValueOnce(new Error("Probe failed"));
    const result = await appCapabilities({ harness: "pi", model: "model-34" });
    expect(result.harnesses[0].catalog).toEqual({
      source: "live",
      refresh: { status: "failed", error: "Probe failed" },
    });
    expect(result.harnesses[0].models?.[0].id).toBe("pi:provider/model-34");
    expect(
      (await appCapabilities({})).harnesses.find((h) => h.id === "pi")?.catalog,
    ).toEqual(result.harnesses[0].catalog);
  });
  it.each([
    { harness: null },
    { harness: 2 },
    { harness: "Pi" },
    { model: null },
    { model: 1 },
    { model: "  " },
    { model: "x".repeat(513) },
  ])("rejects invalid filters %j without probing", async (input) => {
    await expect(appCapabilities(input)).rejects.toThrow();
    expect(piRefresh).not.toHaveBeenCalled();
    expect(codexRefresh).not.toHaveBeenCalled();
  });
});
