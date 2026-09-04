import { describe, expect, it } from "vitest";
import {
  flattenOpenCodeModels,
  parseAgentListCliOutput,
  parseModelsCliOutput,
} from "./opencodeCatalog";
import {
  buildOpenCodePermissionRules,
  compareSemver,
  contextUsedFromMessageInfo,
  inferDefaultAgent,
  inferDefaultVariant,
  isOpenCodeDefaultTitle,
  isOpenCodeNotFound,
  mergeOpenCodeAssistantText,
  parseOpenCodeModelSlug,
  parseOpenCodeVersion,
  parseServerUrlFromOutput,
  toOpenCodePermissionReply,
  toolKindFromName,
} from "./opencodeProtocol";

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
    expect(models[1].settings?.some((setting) => setting.id === "variant")).toBe(
      true,
    );
    expect(models[0].settings?.find((setting) => setting.id === "agent")?.value).toBe(
      "build",
    );
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
    expect(
      inferDefaultAgent([{ name: "plan" }, { name: "build" }]),
    ).toBe("build");
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
        tokens: { input: 0, output: 0, reasoning: 0, cache: { read: 0, write: 0 } },
      }),
    ).toBeUndefined();
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
