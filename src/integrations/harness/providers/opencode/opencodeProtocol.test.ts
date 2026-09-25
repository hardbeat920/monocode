import { describe, expect, it } from "vitest";
import {
  flattenOpenCodeModels,
  openCodeProviderName,
  parseAgentListCliOutput,
  parseModelsCliOutput,
  parseV2Agents,
  parseV2Catalog,
} from "./opencodeCatalog";
import {
  assertSupportedOpenCodeVersion,
  buildOpenCodePermissionRules,
  compareSemver,
  contextUsedFromMessageInfo,
  turnMetricsFromMessageInfo,
  detailFromToolPart,
  eventSessionId,
  inferDefaultAgent,
  inferDefaultVariant,
  isOpenCodeDefaultTitle,
  isOpenCodeNotFound,
  mergeOpenCodeAssistantText,
  openCodeVariantLabel,
  parseOpenCodeModelSlug,
  parseOpenCodeVersion,
  parseServerUrlFromOutput,
  sortOpenCodeVariants,
  toOpenCodePermissionReply,
  toolKindFromName,
} from "./opencodeProtocol";

describe("eventSessionId", () => {
  it.each([
    {
      type: "permission.asked",
      properties: { id: "permission_1", sessionID: "session_1" },
    },
    {
      type: "session.created",
      properties: { info: { id: "session_1", parentID: "session_parent" } },
    },
    {
      type: "message.updated",
      properties: { info: { id: "message_1", sessionID: "session_1" } },
    },
    {
      type: "message.part.updated",
      properties: { part: { id: "part_1", sessionID: "session_1" } },
    },
    {
      type: "message.part.delta",
      properties: { sessionID: "session_1", partID: "part_1" },
    },
  ])("extracts the owning session for $type", (event) => {
    expect(eventSessionId(event)).toBe("session_1");
  });

  it("does not mistake message IDs for session IDs", () => {
    expect(
      eventSessionId({
        type: "message.updated",
        properties: { info: { id: "message_1" } },
      }),
    ).toBeUndefined();
  });
});

describe("parseOpenCodeModelSlug", () => {
  it("splits provider/model", () => {
    expect(parseOpenCodeModelSlug("anthropic/claude-sonnet-4-6")).toEqual({
      providerID: "anthropic",
      modelID: "claude-sonnet-4-6",
    });
  });

  it("rejects bare ids", () => {
    expect(parseOpenCodeModelSlug("glm-5")).toBeNull();
    expect(parseOpenCodeModelSlug("/model")).toBeNull();
    expect(parseOpenCodeModelSlug("provider/")).toBeNull();
  });
});

describe("tool kinds", () => {
  it("classifies todo writes as internal task activity", () => {
    expect(toolKindFromName("todowrite")).toBe("tasks");
  });
});

describe("tool failure details", () => {
  it("extracts nested provider errors instead of dropping them", () => {
    expect(
      detailFromToolPart({
        id: "agent-1",
        type: "tool",
        tool: "task",
        state: {
          status: "error",
          error: { data: { message: "worker disconnected" } },
        },
      }),
    ).toBe("worker disconnected");
  });
});

describe("parseServerUrlFromOutput", () => {
  it("reads the listening URL from server output", () => {
    expect(
      parseServerUrlFromOutput(
        "opencode server listening on http://127.0.0.1:4096",
      ),
    ).toBe("http://127.0.0.1:4096");
  });
});

describe("parseOpenCodeVersion / compareSemver", () => {
  it("extracts a semver and gates 1.14.19", () => {
    expect(parseOpenCodeVersion("1.14.19")).toBe("1.14.19");
    expect(parseOpenCodeVersion("opencode 1.15.0")).toBe("1.15.0");
    expect(compareSemver("1.14.18", "1.14.19")).toBeLessThan(0);
    expect(compareSemver("1.14.19", "1.14.19")).toBe(0);
    expect(compareSemver("1.15.0", "1.14.19")).toBeGreaterThan(0);
  });

  it("selects the v1 and v2 API generations and rejects unknown majors", () => {
    expect(assertSupportedOpenCodeVersion("1.14.19")).toBe("v1");
    expect(assertSupportedOpenCodeVersion("2.0.15")).toBe("v2");
    expect(() => assertSupportedOpenCodeVersion("2.0.14")).toThrow(
      "Upgrade to v2.0.15",
    );
    expect(() => assertSupportedOpenCodeVersion("3.0.0")).toThrow(
      "MonoCode supports OpenCode v1 and v2",
    );
  });
});

describe("buildOpenCodePermissionRules", () => {
  it("allows everything in full-access", () => {
    expect(buildOpenCodePermissionRules("full-access")).toEqual([
      { permission: "*", pattern: "*", action: "allow" },
    ]);
  });

  it("asks by default and allows edits in auto-accept-edits", () => {
    const rules = buildOpenCodePermissionRules("auto-accept-edits");
    expect(rules).toContainEqual({
      permission: "edit",
      pattern: "*",
      action: "allow",
    });
    expect(rules[0]).toEqual({
      permission: "*",
      pattern: "*",
      action: "ask",
    });
  });

  it("maps allow/deny onto OpenCode reply values", () => {
    expect(toOpenCodePermissionReply("allow")).toBe("once");
    expect(toOpenCodePermissionReply("deny")).toBe("reject");
  });
});

describe("OpenCode CLI inventory parsers", () => {
  it("parses models --verbose output", () => {
    const stdout = [
      "opencode/glm-5",
      '{"id":"glm-5","name":"GLM 5","variants":{"high":{},"medium":{}}}',
      "anthropic/claude-sonnet-4-6",
      '{"id":"claude-sonnet-4-6","name":"Claude Sonnet 4.6","variants":{"high":{}}}',
      "",
    ].join("\n");
    const parsed = parseModelsCliOutput(stdout);
    const models = flattenOpenCodeModels(parsed, [
      { name: "build", mode: "primary", hidden: false },
      { name: "plan", mode: "primary", hidden: false },
      { name: "title", mode: "primary", hidden: true },
    ]);
    expect(models.map((model) => model.nativeId)).toEqual([
      "anthropic/claude-sonnet-4-6",
      "opencode/glm-5",
    ]);
    expect(models.map((model) => model.provider)).toEqual([
      { id: "anthropic", name: "Anthropic" },
      { id: "opencode", name: "OpenCode" },
    ]);
    expect(
      models[1].settings?.some((setting) => setting.id === "variant"),
    ).toBe(true);
    expect(
      models[0].settings?.find((setting) => setting.id === "agent")?.value,
    ).toBe("build");
  });

  it("parses agent list headers", () => {
    const agents = parseAgentListCliOutput(
      ["build (primary)", "{}", "compaction (primary)", "{}"].join("\n"),
    );
    expect(agents).toEqual([
      { name: "build", mode: "primary", hidden: false },
      { name: "compaction", mode: "primary", hidden: true },
    ]);
  });

  it("sorts variant options and labels xhigh as Extra High", () => {
    const parsed = parseModelsCliOutput(
      [
        "some-cloud/spark-1",
        '{"id":"spark-1","name":"Spark 1","variants":{"high":{},"minimal":{},"xhigh":{},"low":{},"medium":{}}}',
        "",
      ].join("\n"),
    );
    const [model] = flattenOpenCodeModels(parsed, []);
    const variant = model?.settings?.find((setting) => setting.id === "variant");
    expect(variant?.options.map((option) => option.value)).toEqual([
      "minimal",
      "low",
      "medium",
      "high",
      "xhigh",
    ]);
    expect(variant?.options.map((option) => option.label)).toEqual([
      "Minimal",
      "Low",
      "Medium",
      "High",
      "Extra High",
    ]);
    expect(variant?.value).toBe("medium");
  });

  it("uses familiar provider names and readable custom-provider fallbacks", () => {
    expect(openCodeProviderName("opencode-go")).toBe("OpenCode Go");
    expect(openCodeProviderName("openai")).toBe("OpenAI");
    expect(openCodeProviderName("acme-cloud")).toBe("Acme Cloud");
  });

  it("normalizes v2 API models, providers, variants, and agents", () => {
    const parsed = parseV2Catalog(
      [
        {
          id: "anthropic/claude-sonnet-4-6",
          modelID: "claude-sonnet-4-6",
          providerID: "anthropic",
          name: "Claude Sonnet 4.6",
          enabled: true,
          variants: [{ id: "low" }, { id: "high" }],
          limit: { context: 200_000, output: 64_000 },
        },
        {
          id: "disabled/model",
          modelID: "model",
          providerID: "disabled",
          name: "Disabled",
          enabled: false,
          variants: [],
          limit: { context: 1, output: 1 },
        },
      ],
      [{ id: "anthropic", name: "Anthropic API" }],
    );
    const agents = parseV2Agents([
      { id: "build", name: "Build", mode: "primary", hidden: false },
      { id: "title", name: "Title", mode: "primary", hidden: true },
    ]);
    expect(agents.map((agent) => agent.name)).toEqual(["build", "title"]);
    const models = flattenOpenCodeModels(parsed, agents);

    expect(models).toHaveLength(1);
    expect(models[0]).toMatchObject({
      nativeId: "anthropic/claude-sonnet-4-6",
      provider: { id: "anthropic", name: "Anthropic API" },
      contextWindow: 200_000,
    });
    expect(
      models[0]?.settings?.find((setting) => setting.id === "variant"),
    ).toMatchObject({
      value: "high",
      options: [{ value: "low" }, { value: "high" }],
    });
    expect(
      models[0]?.settings?.find((setting) => setting.id === "agent"),
    ).toMatchObject({ value: "build", options: [{ value: "build" }] });
  });
});

describe("mergeOpenCodeAssistantText", () => {
  it("emits only the new suffix", () => {
    expect(mergeOpenCodeAssistantText("Hel", "Hello")).toEqual({
      latestText: "Hello",
      deltaToEmit: "lo",
    });
  });

  it("keeps a longer snapshot if the next update shrinks", () => {
    expect(mergeOpenCodeAssistantText("Hello world", "Hello")).toEqual({
      latestText: "Hello world",
      deltaToEmit: "",
    });
  });
});

describe("OpenCode helpers", () => {
  it("ignores OpenCode placeholder titles", () => {
    expect(
      isOpenCodeDefaultTitle("New session - 2026-08-16T07:24:01.000Z"),
    ).toBe(true);
    expect(isOpenCodeDefaultTitle("Fix login timeout")).toBe(false);
  });

  it("detects 404 / NotFoundError", () => {
    expect(isOpenCodeNotFound({ status: 404 })).toBe(true);
    expect(isOpenCodeNotFound({ name: "NotFoundError" })).toBe(true);
    expect(isOpenCodeNotFound({ status: 500, name: "NotFoundError" })).toBe(
      false,
    );
  });

  it("infers default variant and agent", () => {
    expect(inferDefaultVariant("anthropic", ["low", "high"])).toBe("high");
    expect(inferDefaultVariant("openai", ["low", "medium", "high"])).toBe(
      "medium",
    );
    expect(inferDefaultAgent([{ name: "plan" }, { name: "build" }])).toBe(
      "build",
    );
  });

  it("prefers medium/high variants on any provider", () => {
    expect(inferDefaultVariant("some-cloud", ["low", "medium", "high"])).toBe(
      "medium",
    );
    expect(inferDefaultVariant("some-cloud", ["low", "high"])).toBe("high");
    expect(inferDefaultVariant("some-cloud", ["low", "xhigh"])).toBeUndefined();
  });

  it("labels variants like Codex/Cursor effort levels", () => {
    expect(openCodeVariantLabel("xhigh")).toBe("Extra High");
    expect(openCodeVariantLabel("extra-high")).toBe("Extra High");
    expect(openCodeVariantLabel("minimal")).toBe("Minimal");
    expect(openCodeVariantLabel("high")).toBe("High");
  });

  it("sorts variants from lowest to highest effort", () => {
    expect(sortOpenCodeVariants(["high", "minimal", "xhigh", "low", "medium"])).toEqual([
      "minimal",
      "low",
      "medium",
      "high",
      "xhigh",
    ]);
  });
});

describe("contextUsedFromMessageInfo", () => {
  it("counts cache reads and writes alongside input and output", () => {
    expect(
      contextUsedFromMessageInfo({
        role: "assistant",
        modelID: "big-pickle",
        providerID: "opencode",
        tokens: {
          input: 1_200,
          output: 800,
          reasoning: 200,
          cache: { read: 40_000, write: 5_000 },
        },
      }),
    ).toBe(47_200);
  });

  it("ignores a message that carries no token block", () => {
    expect(contextUsedFromMessageInfo({ role: "assistant" })).toBeUndefined();
    expect(contextUsedFromMessageInfo(null)).toBeUndefined();
  });

  it("treats an all-zero reading as nothing to report", () => {
    expect(
      contextUsedFromMessageInfo({
        tokens: {
          input: 0,
          output: 0,
          reasoning: 0,
          cache: { read: 0, write: 0 },
        },
      }),
    ).toBeUndefined();
  });

  it("normalizes cache usage for a turn tooltip", () => {
    expect(
      turnMetricsFromMessageInfo({
        tokens: {
          input: 1_200,
          output: 800,
          reasoning: 200,
          cache: { read: 40_000, write: 5_000 },
        },
      }),
    ).toEqual({
      inputTokens: 1_200,
      outputTokens: 1_000,
      cacheReadTokens: 40_000,
      cacheWriteTokens: 5_000,
      cacheHitPercent: (40_000 / 46_200) * 100,
    });
  });
});

describe("flattenOpenCodeModels context window", () => {
  it("carries limit.context onto the catalog entry", () => {
    const models = flattenOpenCodeModels(
      {
        providers: new Map([
          [
            "opencode",
            {
              id: "opencode",
              name: "opencode",
              models: {
                "big-pickle": {
                  id: "big-pickle",
                  name: "Big Pickle",
                  limit: { context: 200_000, output: 32_000 },
                },
              },
            },
          ],
        ]),
        connected: ["opencode"],
      },
      [],
    );
    expect(models[0]?.contextWindow).toBe(200_000);
  });
});
