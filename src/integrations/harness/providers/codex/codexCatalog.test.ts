import { beforeEach, describe, expect, it, vi } from "vitest";

const mock = vi.hoisted(() => ({
  handlers: new Map<string, (line: string) => void>(),
  sent: [] as { method: string; params?: Record<string, unknown> }[],
  config: {} as Record<string, unknown>,
  configError: false,
  pages: [{ data: [] as unknown[], nextCursor: null as string | null }],
  spawn: vi.fn(async (..._args: unknown[]) => undefined),
  kill: vi.fn(async (_id: string) => undefined),
}));

vi.mock("../../../../platform/tauri/fs", () => ({
  homeDir: async () => "/home/user",
}));

vi.mock("../../core/child", () => ({
  resolveCodexBinary: async () => ({ path: "/fake/codex" }),
  spawnChild: mock.spawn,
  killChild: mock.kill,
  watchChild: (id: string, line: (line: string) => void) =>
    mock.handlers.set(id, line),
  unwatchChild: (id: string) => mock.handlers.delete(id),
  writeChild: async (childId: string, line: string) => {
    const request = JSON.parse(line);
    mock.sent.push(request);
    if (request.id === undefined) return;
    if (request.method === "config/read" && mock.configError) {
      mock.handlers.get(childId)?.(
        JSON.stringify({ id: request.id, error: { message: "Unsupported" } }),
      );
      return;
    }
    let result: unknown = {};
    if (request.method === "account/read") {
      result = { account: null, requiresOpenaiAuth: false };
    } else if (request.method === "config/read") {
      result = { config: mock.config };
    } else if (request.method === "model/list") {
      result = mock.pages[request.params?.cursor ? 1 : 0];
    }
    mock.handlers.get(childId)?.(JSON.stringify({ id: request.id, result }));
  },
}));

import { discoverCodexModels } from "./codexCatalog";

beforeEach(() => {
  mock.handlers.clear();
  mock.sent.length = 0;
  mock.config = {};
  mock.configError = false;
  mock.pages = [
    { data: [{ model: "gpt-default", isDefault: true }], nextCursor: null },
  ];
  mock.spawn.mockClear();
  mock.kill.mockClear();
});

describe("discoverCodexModels", () => {
  it.each([
    "commandcode/meta/muse-spark-1.3-contributor",
    "zai-coding/glm-5.3",
  ])(
    "includes the configured custom-provider model %s omitted by model/list",
    async (model) => {
      mock.config = { model_provider: "codex-router", model };
      const models = await discoverCodexModels("/project");
      expect(models.map((entry) => entry.nativeId)).toEqual([
        model,
        "gpt-default",
      ]);
      expect(models[0]).toMatchObject({
        id: `codex:${model}`,
        harness: "codex",
      });
      expect(models[0].settings).toBeUndefined();
      expect(
        mock.sent.find((request) => request.method === "config/read")?.params,
      ).toEqual({});
      expect(mock.handlers.size).toBe(0);
      expect(mock.kill).toHaveBeenCalledOnce();
    },
  );

  it("uses only the reasoning effort reported for the configured model", async () => {
    mock.config = {
      model_provider: "codex-router",
      model: "zai-coding/glm-5.3",
      model_reasoning_effort: "high",
    };
    expect((await discoverCodexModels())[0].settings).toEqual([
      {
        id: "reasoningEffort",
        label: "Reasoning",
        kind: "select",
        value: "high",
        options: [{ value: "high", label: "High" }],
      },
    ]);
  });

  it("preserves paginated catalog settings and default ordering for an existing model", async () => {
    mock.config = {
      model_provider: "codex-router",
      model: "zai-coding/glm-5.3",
      model_reasoning_effort: "high",
    };
    mock.pages = [
      {
        data: [
          {
            model: "zai-coding/glm-5.3",
            supportedReasoningEfforts: ["low", "high"],
            defaultReasoningEffort: "low",
          },
        ],
        nextCursor: "page-2",
      },
      {
        data: [
          { model: "gpt-default", isDefault: true },
          { model: "zai-coding/glm-5.3" },
        ],
        nextCursor: null,
      },
    ];
    const models = await discoverCodexModels();
    expect(models.map((entry) => entry.nativeId)).toEqual([
      "gpt-default",
      "zai-coding/glm-5.3",
    ]);
    expect(models[1].settings?.[0]).toMatchObject({
      value: "low",
      options: [
        { value: "low", label: "Low" },
        { value: "high", label: "High" },
      ],
    });
  });

  it.each(["openai", undefined, ""])(
    "does not add models for the native or unspecified provider %s",
    async (provider) => {
      mock.config = { model_provider: provider, model: "gpt-unknown" };
      expect(
        (await discoverCodexModels()).map((entry) => entry.nativeId),
      ).toEqual(["gpt-default"]);
    },
  );

  it("keeps catalog discovery working when config/read is unavailable", async () => {
    mock.configError = true;
    expect(
      (await discoverCodexModels()).map((entry) => entry.nativeId),
    ).toEqual(["gpt-default"]);
  });

  it("ignores an unset configured model", async () => {
    mock.config = {
      model_provider: "codex-router",
      model: null,
      model_reasoning_effort: null,
    };
    expect(
      (await discoverCodexModels()).map((entry) => entry.nativeId),
    ).toEqual(["gpt-default"]);
  });
});
