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
              models: [
                {
                  model_id: "muse-spark-1.3",
                  display_label: "muse-spark-1.3",
                  visibility: "visible",
                  context_limit: 1007997,
                  reasoning_effort_variants: [
                    { tier: "minimal" },
                    { tier: "low" },
                    { tier: "medium" },
                    { tier: "high" },
                    { tier: "xhigh" },
                    { tier: "max" },
                    { tier: "ultra" },
                  ],
                },
                {
                  model_id: "muse-spark-1.3-contributor",
                  display_label: "muse-spark-1.3-contributor",
                  visibility: "visible",
                  context_limit: 1007997,
                  reasoning_effort_variants: [
                    { tier: "minimal" },
                    { tier: "low" },
                    { tier: "medium" },
                    { tier: "high" },
                    { tier: "xhigh" },
                    { tier: "max" },
                  ],
                },
                {
                  model_id: "muse-spark-retired",
                  display_label: "retired",
                  visibility: "hidden",
                  context_limit: 1,
                  reasoning_effort_variants: [{ tier: "low" }],
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
    expect(models.map((model) => model.id)).toEqual([
      "muse:muse-spark-1.3",
      "muse:muse-spark-1.3-contributor",
    ]);
    expect(models[0]).toMatchObject({
      harness: "muse",
      name: "Muse Spark 1.3",
      nativeId: "muse-spark-1.3",
      contextWindow: 1007997,
    });
    expect(models[1]?.name).toBe("Muse Spark 1.3 (Contributor)");

    // `ultra` is undocumented and `max` is Standard-tier only.
    const standard = models[0]?.settings?.[0];
    expect(standard?.options.map((option) => option.value)).toEqual([
      "minimal",
      "low",
      "medium",
      "high",
      "xhigh",
      "max",
    ]);
    expect(standard?.value).toBe("high");
    const contributor = models[1]?.settings?.[0];
    expect(contributor?.options.map((option) => option.value)).toEqual([
      "minimal",
      "low",
      "medium",
      "high",
      "xhigh",
    ]);
  });

  it("falls back to documented tiers when the list carries no variants", async () => {
    const { effortChoicesForRow } = await import("./museCatalog");
    expect(effortChoicesForRow({ model_id: "muse-spark-9" }).map((c) => c.value)).toEqual([
      "minimal",
      "low",
      "medium",
      "high",
      "xhigh",
      "max",
    ]);
    expect(
      effortChoicesForRow({ model_id: "muse-spark-9-contributor" }).map((c) => c.value),
    ).toEqual(["minimal", "low", "medium", "high", "xhigh"]);
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
