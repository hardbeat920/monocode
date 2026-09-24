import { afterEach, describe, expect, it, vi } from "vitest";

let onLine: ((line: string) => void) | undefined;
let onExit: ((code: number | null) => void) | undefined;
let failSpawn = false;
const written: string[] = [];

vi.mock("../../core/child", () => ({
  resolveMuseBinary: async () => ({ path: "/fake/muse" }),
  spawnChild: async () => {
    if (failSpawn) throw new Error("nope");
  },
  killChild: async () => undefined,
  unwatchChild: () => undefined,
  watchChild: (_id: string, line: (l: string) => void, exit: (c: number | null) => void) => {
    onLine = line;
    onExit = exit;
  },
  writeChild: async (_id: string, line: string) => {
    written.push(line);
    const message = JSON.parse(line) as { id?: number; method?: string };
    // Answer like `muse serve`: handshake, then the provider catalog.
    queueMicrotask(() => {
      if (message.method === "initialize") {
        onLine!(JSON.stringify({ jsonrpc: "2.0", id: message.id, result: {} }));
      } else if (message.method === "model/list") {
        onLine!(
          JSON.stringify({
            jsonrpc: "2.0",
            id: message.id,
            result: {
              providerId: "meta",
              profileId: null,
              source: "providerCatalog",
              models: [
                {
                  modelId: "muse-spark-1.3",
                  displayLabel: "muse-spark-1.3",
                  contextLimit: 1007997,
                  isDefault: false,
                  isActive: false,
                  providerId: "meta",
                  profileId: null,
                  releaseDate: "2026-09-02",
                  outputLimit: 128000,
                  cost: null,
                  description: null,
                },
                {
                  modelId: "muse-spark-1.3-contributor",
                  displayLabel: "muse-spark-1.3-contributor",
                  contextLimit: 1007997,
                  isDefault: true,
                  isActive: false,
                  providerId: "meta",
                  profileId: null,
                  releaseDate: "2026-09-02",
                  outputLimit: 128000,
                  cost: null,
                  description: null,
                },
                {
                  modelId: "muse-spark-1.2",
                  displayLabel: "muse-spark-1.2",
                  contextLimit: 1007997,
                  isDefault: false,
                  isActive: false,
                  providerId: "meta",
                  profileId: null,
                  releaseDate: "2026-08-05",
                  outputLimit: 128000,
                  cost: null,
                  description: null,
                },
                {
                  modelId: "muse-spark-1.2-contributor",
                  displayLabel: "muse-spark-1.2-contributor",
                  contextLimit: 1007997,
                  isDefault: false,
                  isActive: false,
                  providerId: "meta",
                  profileId: null,
                  releaseDate: "2026-08-05",
                  outputLimit: 128000,
                  cost: null,
                  description: null,
                },
                {
                  modelId: "muse-spark-retired",
                  displayLabel: "retired",
                  contextLimit: 1,
                  isDefault: false,
                  isActive: false,
                  visibility: "hidden",
                  providerId: "meta",
                  profileId: null,
                  releaseDate: null,
                  outputLimit: null,
                  cost: null,
                  description: null,
                },
              ],
            },
          }),
        );
      }
    });
  },
}));

vi.mock("../../../../platform/tauri/fs", () => ({
  homeDir: async () => "/tmp/work",
}));

const { refreshMuseCatalog } = await import("./museCatalog");
const {
  modelsFor,
  hasLiveCatalog,
  defaultModelId,
  resetHarnessModelOverlays,
} = await import("../../../../features/sessions/model/models");

afterEach(() => {
  resetHarnessModelOverlays();
  written.length = 0;
  onLine = undefined;
  onExit = undefined;
  failSpawn = false;
});

describe("muse catalog", () => {
  it("replaces the static fallback with the live CLI catalog", async () => {
    expect(hasLiveCatalog("muse")).toBe(false);
    await refreshMuseCatalog();
    expect(hasLiveCatalog("muse")).toBe(true);

    const models = modelsFor("muse");
    expect(models.length).toBeGreaterThan(0);
    // The live catalog flags the contributor model as default and is sorted
    // with it first, mirroring `muse serve` on the installed CLI.
    expect(models.map((model) => model.id)).toEqual([
      "muse:muse-spark-1.3-contributor",
      "muse:muse-spark-1.3",
      "muse:muse-spark-1.2",
      "muse:muse-spark-1.2-contributor",
    ]);
    expect(defaultModelId("muse")).toBe("muse:muse-spark-1.3-contributor");
    expect(models[0]).toMatchObject({
      harness: "muse",
      name: "Muse Spark 1.3 (Contributor)",
      nativeId: "muse-spark-1.3-contributor",
      contextWindow: 1007997,
    });
    expect(models[1]?.name).toBe("Muse Spark 1.3");

    const contributor = models[0]?.settings?.[0];
    expect(contributor?.options.map((option) => option.value)).toEqual([
      "minimal",
      "low",
      "medium",
      "high",
      "xhigh",
    ]);
    expect(contributor?.options.map((option) => option.value)).not.toContain("max");
    expect(contributor?.value).toBe("high");
    const standard = models[1]?.settings?.[0];
    expect(standard?.options.map((option) => option.value)).toEqual([
      "minimal",
      "low",
      "medium",
      "high",
      "xhigh",
      "max",
    ]);
    expect(standard?.options.map((option) => option.value)).not.toContain("none");
    expect(standard?.options.map((option) => option.value)).not.toContain("ultra");
    expect(standard?.value).toBe("high");
    // Spark 1.2 rows get the documented tiers without `max`.
    const spark12 = models[2]?.settings?.[0];
    expect(spark12?.options.map((option) => option.value)).toEqual([
      "minimal",
      "low",
      "medium",
      "high",
      "xhigh",
    ]);
  });

  it("falls back to documented tiers when the list carries no variants", async () => {
    const { effortChoicesForRow } = await import("./museCatalog");
    expect(effortChoicesForRow({ modelId: "muse-spark-1.3" }).map((c) => c.value)).toEqual([
      "minimal",
      "low",
      "medium",
      "high",
      "xhigh",
      "max",
    ]);
    expect(effortChoicesForRow({ modelId: "muse-spark-1.2" }).map((c) => c.value)).toEqual([
      "minimal",
      "low",
      "medium",
      "high",
      "xhigh",
    ]);
    expect(
      effortChoicesForRow({ modelId: "muse-spark-1.2-contributor" }).map((c) => c.value),
    ).toEqual(["minimal", "low", "medium", "high", "xhigh"]);
    expect(
      effortChoicesForRow({
        modelId: "muse-spark-1.2",
        reasoningEffortVariants: [
          { tier: "none" },
          { tier: "ultra" },
          { tier: "high" },
          { tier: "max" },
        ],
      }).map((c) => c.value),
    ).toEqual(["high"]);
    expect(
      effortChoicesForRow({
        modelId: "muse-spark-1.3",
        reasoningEffortVariants: [{ tier: "high" }, { tier: "max" }, { tier: "ultra" }],
      }).map((c) => c.value),
    ).toEqual(["high", "max"]);
    expect(
      effortChoicesForRow({
        modelId: "muse-spark-1.3-contributor",
        reasoningEffortVariants: [{ tier: "high" }, { tier: "max" }, { tier: "ultra" }],
      }).map((c) => c.value),
    ).toEqual(["high"]);
  });

  it("keeps the fallback when the probe fails", async () => {
    failSpawn = true;
    await refreshMuseCatalog();
    expect(hasLiveCatalog("muse")).toBe(false);
    expect(modelsFor("muse").map((model) => model.id)).toEqual([
      "muse:muse-spark-1.3-contributor",
      "muse:muse-spark-1.3",
    ]);
  });
});
