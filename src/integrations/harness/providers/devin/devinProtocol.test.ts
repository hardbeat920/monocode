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
  devinPermissionCommand,
  devinSessionTitle,
  devinStartupError,
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
      "C:\\Users\\me/.config/devin/credentials.toml",
      "C:\\Users\\me/AppData/Roaming/devin/credentials.toml",
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
