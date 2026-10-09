import { describe, expect, it } from "vitest";
import {
  DEVIN_ASK_EDITS_RULE,
  DEVIN_AUTH_HELP,
  devinApiKeyFromCredentials,
  devinAskEditsConfig,
  devinAsksBeforeEdits,
  devinAuthenticateParams,
  devinAuthMethodId,
  devinCommandsFromUpdate,
  devinCredentialsCandidates,
  devinCredentialsPathFromStatus,
  devinEventsFromUpdate,
  devinModeId,
  devinModelFamilies,
  devinModelValue,
  devinNearestEffort,
  devinPermissionCommand,
  devinRestoreError,
  devinSessionMissing,
  devinSessionTitle,
  devinStartupError,
  devinTurnError,
  modelsFromDevinSession,
  stripAnsi,
} from "./devinProtocol";

const update = (body: Record<string, unknown>) => ({
  sessionId: "s1",
  update: body,
});

describe("devin auth helpers", () => {
  it("reads the credentials path from `devin auth status`", () => {
    expect(
      devinCredentialsPathFromStatus(
        "Not logged in.\n  Credentials path: C:\\Users\\me\\AppData\\Roaming\\devin\\credentials.toml\nRun `devin auth login`.",
      ),
    ).toBe("C:\\Users\\me\\AppData\\Roaming\\devin\\credentials.toml");
    expect(devinCredentialsPathFromStatus("Logged in as me")).toBeNull();
    // Devin 3000.6.7 prints a Credentials block instead.
    expect(
      devinCredentialsPathFromStatus(
        "Logged in (via Devin).\r\n\r\nCredentials:\r\n  File:              C:\\Users\\me\\AppData\\Roaming\\devin\\credentials.toml\r\n  API server:        https://server.example\r\n",
      ),
    ).toBe("C:\\Users\\me\\AppData\\Roaming\\devin\\credentials.toml");
  });

  it("handles Windows line endings and POSIX paths alike", () => {
    expect(
      devinCredentialsPathFromStatus(
        "Logged in.\r\n  Credentials path: C:\\Users\\me\\AppData\\Roaming\\devin\\credentials.toml\r\n",
      ),
    ).toBe("C:\\Users\\me\\AppData\\Roaming\\devin\\credentials.toml");
    expect(
      devinCredentialsPathFromStatus(
        "Logged in.\n  Credentials path: /home/me/.config/devin/credentials.toml\n",
      ),
    ).toBe("/home/me/.config/devin/credentials.toml");
    expect(
      devinApiKeyFromCredentials('windsurf_api_key = "devin-key"\r\napi_server_url = "x"\r\n'),
    ).toBe("devin-key");
    expect(devinCredentialsCandidates("C:\\Users\\me\\")).toEqual([
      "C:\\Users\\me/AppData/Roaming/devin/credentials.toml",
      "C:\\Users\\me/.local/share/devin/credentials.toml",
      "C:\\Users\\me/.config/devin/credentials.toml",
    ]);
    // Current CLIs log in to the XDG data directory on macOS and Linux.
    expect(devinCredentialsCandidates("/home/me")).toContain(
      "/home/me/.local/share/devin/credentials.toml",
    );
  });

  it("adds the Supervised edit rule without dropping the user's config", () => {
    expect(devinAsksBeforeEdits("supervised")).toBe(true);
    expect(devinAsksBeforeEdits("auto-accept-edits")).toBe(false);
    const merged = JSON.parse(
      devinAskEditsConfig(
        '{"theme_mode":"dark","permissions":{"allow":["Exec(python3)"],"ask":["exec"]}}',
      ),
    );
    expect(merged.theme_mode).toBe("dark");
    expect(merged.permissions).toEqual({
      allow: ["Exec(python3)"],
      ask: ["exec", DEVIN_ASK_EDITS_RULE],
    });
    expect(JSON.parse(devinAskEditsConfig(null)).permissions.ask).toEqual([
      DEVIN_ASK_EDITS_RULE,
    ]);
    expect(
      JSON.parse(devinAskEditsConfig(JSON.stringify(merged))).permissions.ask,
    ).toEqual(["exec", DEVIN_ASK_EDITS_RULE]);
    // Fails closed rather than starting Supervised without the rule.
    expect(() => devinAskEditsConfig("{ // comment\n}")).toThrow(/not valid JSON/);
    expect(() => devinAskEditsConfig("[]")).toThrow(/not a JSON object/);
    expect(() => devinAskEditsConfig('{"permissions":{"ask":"x"}}')).toThrow(/not a list/);
  });

  it("only treats a vanished conversation as safe to replace", () => {
    expect(devinSessionMissing(new Error("Session not found"))).toBe(true);
    expect(devinSessionMissing(new Error("Method not found"))).toBe(false);
    expect(devinSessionMissing(new Error("devin request timed out"))).toBe(false);
    expect(devinRestoreError(new Error("session/load timed out")).message).toMatch(
      /timed out restoring/,
    );
  });

  it("extracts the stored API key without other fields", () => {
    const toml = [
      'windsurf_api_key = "devin-secret-value"',
      'api_server_url = "https://server.example"',
    ].join("\n");
    expect(devinApiKeyFromCredentials(toml)).toBe("devin-secret-value");
    expect(devinApiKeyFromCredentials("api_server_url = \"x\"")).toBeNull();
    expect(devinApiKeyFromCredentials("windsurf_api_key = ''")).toBeNull();
  });

  it("passes the key on the advertised method's _meta", () => {
    expect(
      devinAuthMethodId({ authMethods: [{ id: "devin-browser" }] }),
    ).toBe("devin-browser");
    expect(devinAuthMethodId({})).toBe("devin-browser");
    expect(devinAuthenticateParams("devin-browser", "k")).toEqual({
      methodId: "devin-browser",
      _meta: { api_key: "k" },
    });
    expect(devinAuthenticateParams("devin-browser", null)).toEqual({
      methodId: "devin-browser",
    });
  });

  it("adds login help to authentication failures only", () => {
    expect(devinStartupError(new Error("ACP host has not authenticated")).message)
      .toContain(DEVIN_AUTH_HELP);
    expect(devinStartupError(new Error("boom")).message).toBe(
      "Devin did not start. boom",
    );
  });

  it("does not blame the login when a running turn times out", () => {
    expect(devinTurnError(new Error("session/prompt timed out")).message).toBe(
      "Devin stopped responding before the turn finished.",
    );
    expect(devinTurnError(new Error("boom")).message).toBe("boom");
    expect(devinTurnError(new Error("api key expired")).message).toContain(
      DEVIN_AUTH_HELP,
    );
  });
});

describe("devin modes", () => {
  it("maps MonoCode access levels onto Devin session modes", () => {
    expect(devinModeId("supervised")).toBe("accept-edits");
    expect(devinModeId("auto-accept-edits")).toBe("accept-edits");
    expect(devinModeId("auto")).toBe("smart");
    expect(devinModeId("full-access")).toBe("bypass");
    expect(devinModeId("full-access", true)).toBe("plan");
  });
});

describe("devin catalog", () => {
  it("reads models from the model config option, current first", () => {
    const models = modelsFromDevinSession({
      configOptions: [
        { id: "mode", category: "mode", options: [{ value: "plan", name: "Plan" }] },
        {
          id: "model",
          category: "model",
          type: "select",
          currentValue: "glm-5-2",
          options: [
            { value: "adaptive", name: "Adaptive" },
            { value: "glm-5-2", name: "GLM-5.2 High" },
            { value: "adaptive", name: "Duplicate" },
          ],
        },
      ],
    });
    expect(models).toEqual([
      { id: "devin:glm-5-2", harness: "devin", name: "GLM-5.2 High", nativeId: "glm-5-2" },
      { id: "devin:adaptive", harness: "devin", name: "Adaptive", nativeId: "adaptive" },
    ]);
  });

  // Names as Devin 3000.6.7 lists them.
  const CHOICES = [
    { value: "adaptive", label: "Adaptive" },
    { value: "claude-opus-5-5-medium", label: "Claude Opus 5.5 Medium" },
    { value: "claude-opus-5-5-low", label: "Claude Opus 5.5 Low" },
    { value: "claude-opus-5-5-max", label: "Claude Opus 5.5 Max" },
    { value: "claude-opus-5-5-low-fast", label: "Claude Opus 5.5 Low Fast" },
    { value: "claude-opus-5-5-max-fast", label: "Claude Opus 5.5 Max Fast" },
    { value: "gpt-6-sol-medium", label: "GPT-6 Sol Medium Thinking" },
    { value: "gpt-6-sol-none", label: "GPT-6 Sol No Thinking" },
    { value: "gpt-6-sol-none-priority", label: "GPT-6 Sol No Thinking Fast" },
    { value: "swe-1-7-lightning", label: "SWE-1.7 Lightning Max" },
    { value: "swe-1-7-lightning-medium", label: "SWE-1.7 Lightning Medium" },
    { value: "glm-5-2-1m", label: "GLM-5.2 High 1M" },
    { value: "glm-5-2-none-1m", label: "GLM-5.2 No Thinking 1M" },
    { value: "claude-opus-4-6", label: "Claude Opus 4.6" },
    { value: "claude-opus-4-6-thinking", label: "Claude Opus 4.6 Thinking" },
  ];

  it("groups effort and speed variants into one model with settings", () => {
    const models = modelsFromDevinSession({
      configOptions: [
        {
          id: "model",
          currentValue: "claude-opus-5-5-medium",
          options: CHOICES.map(({ value, label }) => ({ value, name: label })),
        },
      ],
    });
    expect(models.map((model) => model.name)).toEqual([
      "Claude Opus 5.5",
      "Adaptive",
      "GPT-6 Sol",
      "SWE-1.7 Lightning",
      "GLM-5.2 1M",
      "Claude Opus 4.6",
      "Claude Opus 4.6 Thinking",
    ]);
    expect(models[0]).toMatchObject({
      id: "devin:claude-opus-5-5",
      nativeId: "claude-opus-5-5",
      settings: [
        {
          id: "effort",
          kind: "select",
          value: "medium",
          options: [
            { value: "low", label: "Low" },
            { value: "medium", label: "Medium" },
            { value: "max", label: "Max" },
          ],
        },
        { id: "fast", kind: "toggle", value: "false" },
      ],
    });
    expect(models[1].settings).toBeUndefined();
    // "Thinking" without an effort is a distinct model, not a variant.
    expect(models[6].nativeId).toBe("claude-opus-4-6-thinking");
    expect(models[6].settings).toBeUndefined();
  });

  it("maps a model and its settings back to Devin's exact value", () => {
    const families = devinModelFamilies(CHOICES);
    expect(devinModelValue(families, "claude-opus-5-5")).toBe("claude-opus-5-5-medium");
    expect(devinModelValue(families, "claude-opus-5-5", { effort: "max", fast: "true" }))
      .toBe("claude-opus-5-5-max-fast");
    // No fast variant at this effort: keep the effort, drop the speed.
    expect(devinModelValue(families, "claude-opus-5-5", { effort: "medium", fast: "true" }))
      .toBe("claude-opus-5-5-medium");
    expect(devinModelValue(families, "gpt-6-sol", { effort: "none", fast: "true" }))
      .toBe("gpt-6-sol-none-priority");
    expect(devinModelValue(families, "swe-1-7-lightning", { effort: "max" }))
      .toBe("swe-1-7-lightning");
    expect(devinModelValue(families, "glm-5-2-1m", { effort: "none" })).toBe("glm-5-2-none-1m");
    // Single models and older saved ids pass through untouched.
    expect(devinModelValue(families, "adaptive", { effort: "high" })).toBe("adaptive");
    expect(devinModelValue(families, "claude-opus-5-5-low")).toBe("claude-opus-5-5-low");
  });

  it("keeps ambiguous names as separate models", () => {
    const families = devinModelFamilies([
      { value: "foo", label: "Foo" },
      { value: "foo-max", label: "Foo Max" },
    ]);
    expect(families.map((family) => family.key)).toEqual(["foo", "foo-max"]);
  });

  // Devin 3000.11: effort is a `thought_level` select and names carry no
  // suffix; fusion pairs a lead and sidekick in a single model id.
  const NEW_CATALOG = {
    configOptions: [
      {
        id: "model",
        category: "model",
        type: "select",
        currentValue: "glm-5-2",
        options: [
          { value: "adaptive", name: "Adaptive" },
          { value: "swe-2-high", name: "SWE-2" },
          { value: "swe-1-7-lightning-medium", name: "SWE-1.7 Lightning" },
          { value: "swe-1-6", name: "SWE-1.6" },
          { value: "swe-1-6-fast", name: "SWE-1.6 Fast" },
          { value: "glm-5-2", name: "GLM-5.2 High" },
          {
            value: "fusion-claude-fable-5-1-medium-sidekick-swe-2-medium",
            name: "Fusion (Claude Fable 5.1 Medium + SWE-2 Medium)",
          },
          {
            value: "fusion-gpt-6-sol-high-sidekick-swe-2-medium",
            name: "Fusion (GPT-6 Sol High Thinking + SWE-2 Medium)",
          },
          {
            value: "fusion-claude-fable-5-1-medium-sidekick-glm-5-2",
            name: "Fusion (Claude Fable 5.1 Medium + GLM-5.2 High)",
          },
          {
            value: "fusion-claude-fable-5-1-medium-sidekick-swe-2-high",
            name: "Fusion (Claude Fable 5.1 Medium + SWE-2 High)",
          },
        ],
      },
      {
        id: "thought_level",
        category: "thought_level",
        type: "select",
        currentValue: "high",
        options: [
          { value: "none", name: "No Thinking" },
          { value: "high", name: "High" },
          { value: "max", name: "Max" },
        ],
      },
    ],
  };

  it("surfaces thought_level as the effort select on every model", () => {
    const models = modelsFromDevinSession(NEW_CATALOG);
    const swe2 = models.find((model) => model.nativeId === "swe-2-high")!;
    // Effort lives in its own select, so the name carries none ("SWE-2 High
    // · Max" read as two efforts).
    expect(swe2.name).toBe("SWE-2");
    expect(models.find((model) => model.nativeId === "glm-5-2")!.name).toBe("GLM-5.2");
    expect(swe2.settings).toEqual([
      {
        id: "effort",
        label: "Effort",
        kind: "select",
        value: "high",
        options: [
          { value: "none", label: "No Thinking" },
          { value: "high", label: "High" },
          { value: "max", label: "Max" },
        ],
      },
    ]);
    const adaptive = models.find((model) => model.nativeId === "adaptive")!;
    expect(adaptive.settings?.[0]?.id).toBe("effort");
    // A fast variant still pairs into a toggle.
    const swe16 = models.find((model) => model.nativeId === "swe-1-6")!;
    expect(swe16.settings?.map((setting) => setting.id)).toEqual([
      "effort",
      "fast",
    ]);
  });

  it("uses each model's own effort levels when the probe read them", () => {
    const level = (values: string[], current: string) => ({
      id: "thought_level",
      current,
      choices: values.map((value) => ({ value, label: value })),
    });
    const models = modelsFromDevinSession(
      NEW_CATALOG,
      new Map([
        ["swe-2-high", level(["medium", "high", "max"], "high")],
        ["adaptive", null],
        ["swe-1-6", null],
      ]),
    );
    const byId = (id: string) => models.find((model) => model.nativeId === id)!;
    expect(byId("swe-2-high").settings?.[0]).toMatchObject({
      id: "effort",
      value: "high",
      options: [
        { value: "medium", label: "medium" },
        { value: "high", label: "high" },
        { value: "max", label: "max" },
      ],
    });
    // No levels for this model: no Effort select at all.
    expect(byId("adaptive").settings).toBeUndefined();
    expect(byId("swe-1-6").settings?.map((setting) => setting.id)).toEqual(["fast"]);
    // A model the probe missed falls back to the session's levels.
    expect(byId("glm-5-2").settings?.[0]?.options).toHaveLength(3);
  });

  it("names a fixed id effort only when there is no Effort select", () => {
    const models = modelsFromDevinSession(NEW_CATALOG, new Map([["swe-2-high", null]]));
    expect(models.find((model) => model.nativeId === "swe-2-high")!.name).toBe("SWE-2 High");
  });

  it("maps a saved effort onto the nearest level a model offers", () => {
    expect(devinNearestEffort("max", ["low", "medium", "high", "xhigh", "max"])).toBe("max");
    expect(devinNearestEffort("none", ["medium", "high", "max"])).toBe("medium");
    expect(devinNearestEffort("medium", ["none", "high", "max"])).toBe("high");
    // Equally close: the lower level.
    expect(devinNearestEffort("low", ["none", "high", "max"])).toBe("none");
    expect(devinNearestEffort("max", ["low", "medium", "high"])).toBe("high");
    expect(devinNearestEffort("turbo", ["high"])).toBeNull();
    expect(devinNearestEffort("high", [])).toBeNull();
  });

  it("resolves a family key saved by an older catalog to the listed id", () => {
    const families = devinModelFamilies([{ value: "swe-2-high", label: "SWE-2" }]);
    expect(devinModelValue(families, "swe-2")).toBe("swe-2-high");
  });

  it("collapses fusion ids into one model with lead and sidekick", () => {
    const models = modelsFromDevinSession(NEW_CATALOG);
    const fusions = models.filter((model) => model.name.startsWith("Fusion"));
    expect(fusions).toHaveLength(1);
    const [fusion] = fusions;
    expect(fusion.nativeId).toBe("fusion");
    expect(fusion.name).toBe("Fusion");
    // Each lead is paired at one effort, so there is nothing to choose.
    expect(fusion.settings?.map((setting) => setting.id)).toEqual([
      "lead",
      "sidekick",
    ]);
    const [lead, sidekick] = fusion.settings!;
    expect(lead.options).toEqual([
      { value: "claude-fable-5-1", label: "Claude Fable 5.1" },
      { value: "gpt-6-sol", label: "GPT-6 Sol" },
    ]);
    expect(sidekick.options).toEqual([
      { value: "swe-2-medium", label: "SWE-2 Medium" },
      { value: "glm-5-2", label: "GLM-5.2 High" },
      { value: "swe-2-high", label: "SWE-2 High" },
    ]);
  });

  it("offers a fusion effort only when a lead has more than one", () => {
    const families = devinModelFamilies([
      { value: "fusion-gpt-6-sol-high-sidekick-swe-2-medium", label: "Fusion (GPT-6 Sol High Thinking + SWE-2 Medium)" },
      { value: "fusion-gpt-6-sol-max-sidekick-swe-2-medium", label: "Fusion (GPT-6 Sol Max Thinking + SWE-2 Medium)" },
    ]);
    const models = modelsFromDevinSession({
      configOptions: [
        {
          id: "model",
          options: families[0].variants.map((variant) => ({ value: variant.value, name: variant.name })),
        },
      ],
    });
    expect(models[0].settings?.map((setting) => setting.id)).toEqual(["lead", "effort", "sidekick"]);
    expect(devinModelValue(families, "fusion", { effort: "max" })).toBe(
      "fusion-gpt-6-sol-max-sidekick-swe-2-medium",
    );
  });

  it("maps fusion settings back to the exact catalog id", () => {
    const families = devinModelFamilies(
      NEW_CATALOG.configOptions[0].options.map((option) => ({
        value: option.value,
        label: option.name,
      })),
    );
    expect(devinModelValue(families, "fusion")).toBe(
      "fusion-claude-fable-5-1-medium-sidekick-swe-2-medium",
    );
    expect(devinModelValue(families, "fusion", { lead: "gpt-6-sol" })).toBe(
      "fusion-gpt-6-sol-high-sidekick-swe-2-medium",
    );
    // GPT-6 Sol has no medium variant: keep its real effort.
    expect(
      devinModelValue(families, "fusion", { lead: "gpt-6-sol", effort: "medium" }),
    ).toBe("fusion-gpt-6-sol-high-sidekick-swe-2-medium");
    expect(devinModelValue(families, "fusion", { sidekick: "glm-5-2" })).toBe(
      "fusion-claude-fable-5-1-medium-sidekick-glm-5-2",
    );
    expect(
      devinModelValue(families, "fusion", { sidekick: "swe-2-high" }),
    ).toBe("fusion-claude-fable-5-1-medium-sidekick-swe-2-high");
    // A lead Devin does not pair falls back to the first fusion entry.
    expect(devinModelValue(families, "fusion", { lead: "kimi-k3" })).toBe(
      "fusion-claude-fable-5-1-medium-sidekick-swe-2-medium",
    );
  });
});

describe("devin updates", () => {
  it("strips terminal colour codes from tool output", () => {
    expect(stripAnsi("\u001b[1m\u001b[32mMode\u001b[0m")).toBe("Mode");
    const events = devinEventsFromUpdate(
      update({
        sessionUpdate: "tool_call_update",
        toolCallId: "t1",
        status: "completed",
        content: [{ type: "content", content: { type: "text", text: "\u001b[31mfail\u001b[0m" } }],
      }),
    );
    const tool = events.find((event) => event.type === "tool.updated");
    expect(tool && "detail" in tool ? tool.detail : "").toBe("fail");
  });

  it("surfaces Devin's _meta token accounting as turn metrics", () => {
    const events = devinEventsFromUpdate(
      update({
        sessionUpdate: "usage_update",
        used: 40264,
        size: 200000,
        _meta: {
          "cognition.ai/inputTokens": 100,
          "cognition.ai/outputTokens": 3,
          "cognition.ai/cachedReadTokens": 300,
        },
      }),
    );
    expect(events).toContainEqual({ type: "context", used: 40264, window: 200000 });
    expect(events).toContainEqual({
      type: "turn.metrics",
      inputTokens: 100,
      outputTokens: 3,
      cacheReadTokens: 300,
      cacheHitPercent: 75,
    });
  });

  it("accepts only settled session titles", () => {
    const title = (value: string) =>
      devinSessionTitle(update({ sessionUpdate: "session_info_update", title: value }));
    expect(title("List the files in the current directory, then reply wit...")).toBeNull();
    expect(title('functions.shell:0{"command": "ls"}')).toBeNull();
    expect(title("List current directory files")).toBe("List current directory files");
    expect(devinSessionTitle(update({ sessionUpdate: "plan" }))).toBeNull();
  });

  it("lists built-in commands but leaves skills to disk discovery", () => {
    const commands = devinCommandsFromUpdate(
      update({
        sessionUpdate: "available_commands_update",
        availableCommands: [
          { name: "compact", description: "Force compaction", _meta: { "cognition.ai/category": "Session" } },
          { name: "loop", description: "Loop", input: { hint: "<prompt>" }, _meta: { "cognition.ai/category": "Session" } },
          { name: "remotion-docs", description: "Docs", _meta: { "cognition.ai/category": "Skills" } },
        ],
      }),
    );
    expect(commands).toEqual([
      { name: "compact", description: "Force compaction", invocation: "devin:compact", source: "devin", origin: "Session" },
      { name: "loop", description: "Loop", invocation: "loop", source: "devin", origin: "Session", inputHint: "<prompt>" },
    ]);
  });

  it("reads the command a permission request is about", () => {
    expect(
      devinPermissionCommand({
        toolCall: { toolCallId: "t", _meta: { "cognition.ai/editableCommand": "ls" } },
      }),
    ).toBe("ls");
    expect(devinPermissionCommand({ toolCall: { toolCallId: "t" } })).toBeUndefined();
  });
});
