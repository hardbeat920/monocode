import { afterEach, describe, expect, it, vi } from "vitest";

it.each([
  { variants: ["low"], expected: ["low"] },
  { variants: [], expected: [] },
  { variants: "unknown", expected: [] },
])(
  "honors the complete native effort set $variants",
  async ({ variants, expected }) => {
    const { modelsFromList } = await import("./museCatalog");
    const [model] = modelsFromList({
      models: [
        {
          modelId: "muse-spark-1.3",
          variants,
          defaultReasoningEffort: "low",
          reasoningEffortVariants: [{ tier: "high" }],
        },
      ],
    });
    expect(
      model.settings?.[0]?.options.map((option) => option.value) ?? [],
    ).toEqual(expected);
    if (expected.length) expect(model.settings?.[0]?.value).toBe("low");
  },
);

const watchers = new Map<
  string,
  { line: (line: string) => void; exit: (code: number | null) => void }
>();
const spawnedIds: string[] = [];
let failSpawn = false;
let initializeBehavior: "ok" | "error" | "exit" | "hang" = "ok";
let killed = 0;
let unwatched = 0;
let spawns = 0;
let spawnedCwd: string | undefined;
const homeDir = vi.fn(async () => "/tmp/work");
const written: string[] = [];

vi.mock("../../core/child", () => ({
  resolveMuseBinary: async () => ({ path: "/fake/muse" }),
  spawnChild: async (
    _id: string,
    _command: string,
    _args: string[],
    cwd: string,
  ) => {
    spawnedCwd = cwd;
    spawnedIds.push(_id);
    spawns += 1;
    if (failSpawn) throw new Error("nope");
  },
  killChild: async () => {
    killed += 1;
  },
  unwatchChild: (id: string) => {
    unwatched += 1;
    watchers.delete(id);
  },
  watchChild: (
    _id: string,
    line: (l: string) => void,
    exit: (c: number | null) => void,
  ) => {
    watchers.set(_id, { line, exit });
  },
  writeChild: async (_id: string, line: string) => {
    written.push(line);
    const { line: onLine, exit: onExit } = watchers.get(_id)!;
    const message = JSON.parse(line) as { id?: number; method?: string };
    // Answer like `muse serve`: handshake, then the provider catalog.
    queueMicrotask(() => {
      if (message.method === "initialize") {
        if (initializeBehavior === "hang") return;
        if (initializeBehavior === "exit") {
          onExit(1);
          return;
        }
        if (initializeBehavior === "error") {
          onLine(
            JSON.stringify({
              jsonrpc: "2.0",
              id: message.id,
              error: { message: "not logged in" },
            }),
          );
          return;
        }
        onLine(JSON.stringify({ jsonrpc: "2.0", id: message.id, result: {} }));
      } else if (message.method === "model/list") {
        onLine(
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
  homeDir: () => homeDir(),
}));

const { refreshMuseCatalog } = await import("./museCatalog");
const { modelsFor, hasLiveCatalog, defaultModelId, resetHarnessModelOverlays } =
  await import("../../../../features/sessions/model/models");

afterEach(() => {
  resetHarnessModelOverlays();
  written.length = 0;
  watchers.clear();
  spawnedIds.length = 0;
  failSpawn = false;
  initializeBehavior = "ok";
  killed = 0;
  unwatched = 0;
  spawns = 0;
  spawnedCwd = undefined;
  homeDir.mockClear();
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
    expect(contributor?.options.map((option) => option.value)).not.toContain(
      "max",
    );
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
    expect(standard?.options.map((option) => option.value)).not.toContain(
      "none",
    );
    expect(standard?.options.map((option) => option.value)).not.toContain(
      "ultra",
    );
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
    expect(
      effortChoicesForRow({ modelId: "muse-spark-1.3" }).map((c) => c.value),
    ).toEqual(["minimal", "low", "medium", "high", "xhigh", "max"]);
    expect(
      effortChoicesForRow({ modelId: "muse-spark-1.2" }).map((c) => c.value),
    ).toEqual(["minimal", "low", "medium", "high", "xhigh"]);
    expect(
      effortChoicesForRow({ modelId: "muse-spark-1.2-contributor" }).map(
        (c) => c.value,
      ),
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
        reasoningEffortVariants: [
          { tier: "high" },
          { tier: "max" },
          { tier: "ultra" },
        ],
      }).map((c) => c.value),
    ).toEqual(["high", "max"]);
    expect(
      effortChoicesForRow({
        modelId: "muse-spark-1.3-contributor",
        reasoningEffortVariants: [
          { tier: "high" },
          { tier: "max" },
          { tier: "ultra" },
        ],
      }).map((c) => c.value),
    ).toEqual(["high"]);
  });

  it("probes from a host working directory without desktop APIs", async () => {
    const { discoverMuseModels } = await import("./museCatalog");
    const models = await discoverMuseModels("/tmp/host-work");
    expect(models).toHaveLength(4);
    expect(spawnedCwd).toBe("/tmp/host-work");
    expect(homeDir).not.toHaveBeenCalled();
    expect(killed).toBe(1);
  });

  it("isolates concurrent host catalog probes", async () => {
    const { discoverMuseModels } = await import("./museCatalog");
    const results = await Promise.all([
      discoverMuseModels("/tmp/host-one"),
      discoverMuseModels("/tmp/host-two"),
    ]);
    expect(results.map((models) => models.length)).toEqual([4, 4]);
    expect(new Set(spawnedIds).size).toBe(2);
    expect(watchers.size).toBe(0);
    expect(killed).toBe(2);
    expect(unwatched).toBe(2);
  });

  it("coalesces simultaneous refreshes and cleans up the probe", async () => {
    const first = refreshMuseCatalog();
    expect(refreshMuseCatalog()).toBe(first);
    await first;
    expect(spawns).toBe(1);
    expect(killed).toBe(1);
    expect(unwatched).toBe(1);
    expect(written.map((line) => JSON.parse(line).method)).toEqual([
      "initialize",
      "initialized",
      "model/list",
    ]);
  });

  it.each(["error", "exit"] as const)(
    "cleans up a probe after initialize %s",
    async (behavior) => {
      initializeBehavior = behavior;
      await refreshMuseCatalog();
      expect(hasLiveCatalog("muse")).toBe(false);
      expect(killed).toBe(1);
      expect(unwatched).toBe(1);
      initializeBehavior = "ok";
      await refreshMuseCatalog();
      expect(hasLiveCatalog("muse")).toBe(true);
    },
  );

  it("times out and cleans up an unanswered handshake", async () => {
    vi.useFakeTimers();
    try {
      initializeBehavior = "hang";
      const refresh = refreshMuseCatalog();
      await vi.advanceTimersByTimeAsync(15_001);
      await refresh;
      expect(hasLiveCatalog("muse")).toBe(false);
      expect(killed).toBe(1);
      expect(unwatched).toBe(1);
      expect(vi.getTimerCount()).toBe(0);
    } finally {
      vi.useRealTimers();
    }
  });

  it("keeps the fallback when the probe fails", async () => {
    failSpawn = true;
    await refreshMuseCatalog();
    expect(hasLiveCatalog("muse")).toBe(false);
    expect(modelsFor("muse").map((model) => model.id)).toEqual([
      "muse:default",
    ]);
    expect(killed).toBe(1);
    expect(unwatched).toBe(1);
  });
});
