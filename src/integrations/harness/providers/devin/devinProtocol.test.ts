import { describe, expect, it } from "vitest";
import {
  DEVIN_AUTH_HELP,
  devinApiKeyFromCredentials,
  devinAuthenticateParams,
  devinAuthMethodId,
  devinCommandsFromUpdate,
  devinCredentialsCandidates,
  devinCredentialsPathFromStatus,
  devinEventsFromUpdate,
  devinModeId,
  devinModelFamilies,
  devinModelValue,
  devinPermissionCommand,
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
      "C:\\Users\\me/.config/devin/credentials.toml",
    ]);
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
