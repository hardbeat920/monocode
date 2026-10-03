import { describe, expect, it } from "vitest";
import {
  flattenOpenCodeModels,
  openCodeProviderName,
  parseAgentListCliOutput,
  parseModelsCliOutput,
} from "./opencodeCatalog";
import {
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
  isSupportedOpenCodeVersion,
  managedOpenCodeConfig,
  verifyManagedOpenCodePolicy,
  nextOpenCodeMessageId,
  mergeOpenCodeAssistantText,
  openCodeVariantLabel,
  parseOpenCodeModelSlug,
  parseOpenCodeVersion,
  parseOpenCodeToolOutputGlob,
  parseServerUrlFromOutput,
  sortOpenCodeVariants,
  toOpenCodePermissionReply,
  toolKindFromName,
} from "./opencodeProtocol";

describe("effective managed permissions", () => {
  const rules = (permission: string, pattern: string, action: string) => ({
    permission,
    pattern,
    action,
  });
  const verify = (
    permission: unknown,
    planning = false,
    config: unknown = {},
  ) =>
    verifyManagedOpenCodePolicy(
      [{ name: "custom", mode: "primary", permission }],
      config,
      "supervised",
      planning,
    );
  it("rejects a higher-priority custom allow after the managed ask rule", () => {
    expect(() =>
      verify([rules("*", "*", "ask"), rules("mcp_write", "*", "allow")]),
    ).toThrow("grants tools beyond");
  });
  it("accepts stricter rules that cover the whole earlier allow", () => {
    expect(() =>
      verify([
        rules("*", "*", "ask"),
        rules("mcp_write", "*", "allow"),
        rules("mcp_write", "*", "deny"),
      ]),
    ).not.toThrow();
  });
  it("rejects narrow wildcard denies that do not cover an earlier broad allow", () => {
    expect(() =>
      verify([
        rules("*", "*", "ask"),
        rules("mcp_write", "foo*", "allow"),
        rules("mcp_write", "foo?", "deny"),
      ]),
    ).toThrow("grants tools beyond");
  });
  it("allows only exact readonly Plan exceptions and the explore task", () => {
    expect(() =>
      verify(
        [
          rules("*", "*", "deny"),
          rules("read", "*", "allow"),
          rules("task", "explore", "allow"),
        ],
        true,
      ),
    ).not.toThrow();
    expect(() =>
      verify(
        [rules("*", "*", "deny"), rules("task", "explore*", "allow")],
        true,
      ),
    ).toThrow("grants tools beyond");
    expect(() =>
      verify(
        [rules("*", "*", "deny"), rules("search_and_delete", "*", "allow")],
        true,
      ),
    ).toThrow("grants tools beyond");
  });
  it("requires a safe broad baseline and the ordered rule array", () => {
    expect(() => verify([rules("read", "*", "deny")])).toThrow(
      "grants tools beyond",
    );
    expect(() => verify({ "*": "ask" })).toThrow("grants tools beyond");
    expect(() => verify([rules("*", "*", "ask")], true)).toThrow(
      "grants tools beyond",
    );
  });
  it.each([
    ["build", "primary", true],
    ["plan", "primary", true],
    ["custom", "primary", true],
    ["general", "subagent", false],
    ["explore", "subagent", false],
    ["custom", "all", false],
    ["custom", undefined, false],
    ["explore", "primary", false],
  ] as const)(
    "allows a Plan Explore grant for %s in mode %s only when safe",
    (name, mode, allowed) => {
      const check = () =>
        verifyManagedOpenCodePolicy(
          [
            {
              name,
              mode,
              permission: [
                rules("*", "*", "deny"),
                rules("task", "explore", "allow"),
              ],
            },
          ],
          {},
          "full-access",
          true,
        );
      if (allowed) expect(check).not.toThrow();
      else expect(check).toThrow("grants tools beyond");
    },
  );
  it("accepts a later deny that removes a subagent's Explore grant", () => {
    expect(() =>
      verifyManagedOpenCodePolicy(
        [
          {
            name: "explore",
            mode: "subagent",
            permission: [
              rules("*", "*", "deny"),
              rules("task", "explore", "allow"),
              rules("task", "*", "deny"),
            ],
          },
        ],
        {},
        "full-access",
        true,
      ),
    ).not.toThrow();
  });
  it("allows only the exact host-probed tool-output directory", () => {
    const output = "/isolated/data/opencode/tool-output/*";
    const check = (pattern: string) =>
      verifyManagedOpenCodePolicy(
        [
          {
            name: "build",
            permission: [
              rules("*", "*", "deny"),
              rules("external_directory", pattern, "allow"),
            ],
          },
        ],
        {},
        "supervised",
        true,
        output,
      );
    expect(() => check(output)).not.toThrow();
    expect(() => check("/isolated/data/opencode/*")).toThrow(
      "grants tools beyond",
    );
    expect(() => check("/other/opencode/tool-output/*")).toThrow(
      "grants tools beyond",
    );
    expect(
      buildOpenCodePermissionRules("supervised", true, output),
    ).toContainEqual(rules("external_directory", output, "allow"));
  });
  it("rejects a restored child-tool grant and skips restrictions in full access", () => {
    expect(() =>
      verify([rules("*", "*", "ask")], false, {
        experimental: { primary_tools: ["bash"] },
      }),
    ).toThrow("grants tools beyond");
    expect(() =>
      verifyManagedOpenCodePolicy([], {}, "full-access", false),
    ).not.toThrow();
  });
  it("patches legacy mode only for agents observed as primary", () => {
    const config = managedOpenCodeConfig(
      "build (primary)\n[]\ngeneral (subagent)\n[]\nexplore (subagent)\n[]",
      "supervised",
      false,
    );
    expect(Object.keys(config.mode as Record<string, unknown>)).toEqual([
      "build",
    ]);
    expect(Object.keys(config.agent as Record<string, unknown>)).toEqual([
      "build",
      "general",
      "explore",
    ]);
  });
});

it("generates ordered message IDs with the provider timestamp encoding", () => {
  const timestamp = 1791034039388;
  const first = nextOpenCodeMessageId(timestamp);
  const second = nextOpenCodeMessageId(timestamp);
  expect(first).toMatch(/^msg_[0-9a-f]{12}[0-9A-Za-z]{14}$/);
  expect(first.slice(4, 16)).toBe(
    ((BigInt(timestamp) * 4096n + 1n) & 0xffffffffffffn)
      .toString(16)
      .padStart(12, "0"),
  );
  expect(first < second).toBe(true);
});

describe("owned OpenCode tool-output directory", () => {
  it.each([
    [
      "data       /isolated/data/opencode",
      "/isolated/data/opencode/tool-output/*",
    ],
    [
      "data       C:\\Users\\fixture\\opencode",
      "C:\\Users\\fixture\\opencode\\tool-output\\*",
    ],
    [
      "data       \\\\server\\share\\opencode",
      "\\\\server\\share\\opencode\\tool-output\\*",
    ],
  ])("derives the exact output glob from %s", (output, expected) => {
    expect(parseOpenCodeToolOutputGlob(output)).toBe(expected);
  });
  it.each([
    "cache      /cache",
    "data       relative/opencode",
    "data       /safe/../other",
    "data       /safe/*/opencode",
    "data       /safe/opencode?",
    "data       /one\ndata       /two",
  ])("rejects an ambiguous data directory %s", (output) => {
    expect(() => parseOpenCodeToolOutputGlob(output)).toThrow(
      "safe data directory",
    );
  });
});

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
    const variant = model?.settings?.find(
      (setting) => setting.id === "variant",
    );
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
    expect(
      sortOpenCodeVariants(["high", "minimal", "xhigh", "low", "medium"]),
    ).toEqual(["minimal", "low", "medium", "high", "xhigh"]);
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

describe("managed OpenCode permissions", () => {
  const agents = `build (primary)\n[{"permission":"bash","pattern":"*","action":"allow"}]\ncustom (subagent)\n[{"permission":"mcp_write","pattern":"*","action":"allow"},{"permission":"read","pattern":"*.env","action":"allow"}]`;

  it("overrides custom subagent permissions before the server starts", () => {
    const config = managedOpenCodeConfig(agents, "supervised", false);
    expect(config).toMatchObject({
      permission: { "*": "ask", bash: "ask", mcp_write: "ask", read: "ask" },
      agent: {
        custom: { permission: { "*": "ask", mcp_write: "ask", read: "ask" } },
      },
      experimental: { primary_tools: [] },
    });
  });

  it.each([
    ["full-access", true, "deny"],
    ["supervised", false, "ask"],
  ] as const)(
    "preserves only the trusted output-directory grant for %s with Plan %s",
    (mode, planning, baseline) => {
      const pattern = "/isolated/data/opencode/tool-output/*";
      const output = `build (primary)\n${JSON.stringify([
        { permission: "external_directory", pattern, action: "allow" },
      ])}\ngeneral (subagent)\n${JSON.stringify([
        { permission: "external_directory", pattern, action: "allow" },
      ])}`;
      const trusted = "/host/data/opencode/tool-output/*";
      const config = managedOpenCodeConfig(output, mode, planning, trusted);
      const permission = {
        external_directory: {
          "*": baseline,
          [pattern]: baseline,
          [trusted]: "allow",
        },
      };
      expect(config).toMatchObject({
        permission,
        agent: { build: { permission }, general: { permission } },
        mode: { build: { permission } },
      });
    },
  );

  it.each(["full-access", "auto", "auto-accept-edits"] as const)(
    "preserves Plan restrictions under %s",
    (mode) => {
      const rules = buildOpenCodePermissionRules(mode, true);
      expect(rules[0]).toEqual({
        permission: "*",
        pattern: "*",
        action: "deny",
      });
      expect(rules).not.toContainEqual({
        permission: "edit",
        pattern: "*",
        action: "allow",
      });
      expect(managedOpenCodeConfig(agents, mode, true)).toMatchObject({
        permission: {
          "*": "deny",
          bash: "deny",
          task: "deny",
        },
        agent: {
          build: { permission: { task: { "*": "deny", explore: "allow" } } },
          custom: {
            permission: { task: "deny", mcp_write: "deny", read: "allow" },
          },
        },
      });
    },
  );

  it("grants Plan Explore tasks only to primary agents that cannot be the Explore child", () => {
    const config = managedOpenCodeConfig(
      "build (primary)\n[]\nplan (primary)\n[]\ngeneral (subagent)\n[]\nexplore (primary)\n[]\ncustom (all)\n[]",
      "full-access",
      true,
    );
    const taskAllowed = { task: { "*": "deny", explore: "allow" } };
    const taskDenied = { task: "deny" };
    expect(config).toMatchObject({
      permission: taskDenied,
      agent: {
        build: { permission: taskAllowed },
        plan: { permission: taskAllowed },
        general: { permission: taskDenied },
        explore: { permission: taskDenied },
        custom: { permission: taskDenied },
      },
      mode: {
        build: { permission: taskAllowed },
        plan: { permission: taskAllowed },
        explore: { permission: taskDenied },
      },
    });
    expect(config.mode).not.toHaveProperty("general");
    expect(config.mode).not.toHaveProperty("custom");
    expect(buildOpenCodePermissionRules("full-access", true)).toContainEqual({
      permission: "task",
      pattern: "explore",
      action: "allow",
    });
  });

  it("fails closed when the CLI omits effective agent permissions", () => {
    expect(() =>
      managedOpenCodeConfig("custom (subagent)\nnot JSON", "supervised", false),
    ).toThrow("Could not read OpenCode permissions");
    expect(() => managedOpenCodeConfig("", "supervised", false)).toThrow(
      "did not expose",
    );
  });

  it("denies custom mutating permissions whose names contain read or search", () => {
    const config = managedOpenCodeConfig(
      'custom (primary)\n[{"permission":"spreadsheet_delete","pattern":"*","action":"allow"},{"permission":"search_and_delete","pattern":"*","action":"allow"}]',
      "full-access",
      true,
    );
    expect(config).toMatchObject({
      agent: {
        custom: {
          permission: { spreadsheet_delete: "deny", search_and_delete: "deny" },
        },
      },
    });
  });

  it.each([
    ["1.14.18", false],
    ["1.14.19", true],
    ["1.15.0", true],
    ["2.0.20", false],
  ])("checks the v1 API version %s", (version, supported) => {
    expect(isSupportedOpenCodeVersion(version as string)).toBe(supported);
  });

  it("accepts final text corrections and shortening", () => {
    expect(
      mergeOpenCodeAssistantText("Hello worle", "Hello world", true).latestText,
    ).toBe("Hello world");
    expect(
      mergeOpenCodeAssistantText("Hello world", "Hello", true).latestText,
    ).toBe("Hello");
    expect(mergeOpenCodeAssistantText("Hello", "", true).latestText).toBe("");
  });
});
