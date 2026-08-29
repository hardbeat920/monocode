import { describe, expect, it } from "vitest";
import {
  agentEndWillRetry,
  assistantDeltaFromEvent,
  buildPiPrompt,
  buildPiSpawnArgs,
  buildPiSteer,
  contextFromSessionStats,
  contextFromUsage,
  extensionUiResponse,
  extensionUiTitle,
  isAgentSettled,
  modelsFromRpcData,
  needsExtensionUiReply,
  parseExtensionUiRequest,
  parseJsonLine,
  parsePiModelRef,
  parsePiVersion,
  parseRpcResponse,
  previewFromTool,
  providerSessionIdFromState,
  toolCallStartFromEvent,
  toolExecutionEndFromEvent,
  toolKindFromName,
  toolTitle,
  turnErrorFromEvent,
} from "./piProtocol";
import { OMP_FLAVOR, PI_FLAVOR } from "./piFlavor";

describe("buildPiSpawnArgs", () => {
  it("starts RPC without stripping the user's extensions", () => {
    expect(buildPiSpawnArgs(PI_FLAVOR, {})).toEqual(["--mode", "rpc"]);
    expect(
      buildPiSpawnArgs(PI_FLAVOR, { model: "anthropic/claude-sonnet-4" }),
    ).toEqual(["--mode", "rpc", "--model", "anthropic/claude-sonnet-4"]);
    expect(buildPiSpawnArgs(PI_FLAVOR, { resume: "abc123" })).toEqual([
      "--mode",
      "rpc",
      "--session",
      "abc123",
    ]);
  });

  it("can skip session files without disabling extensions", () => {
    expect(buildPiSpawnArgs(PI_FLAVOR, { noSession: true })).toEqual([
      "--mode",
      "rpc",
      "--no-session",
    ]);
  });

  it("strips extensions for throwaway catalog probes", () => {
    expect(
      buildPiSpawnArgs(PI_FLAVOR, { noSession: true, noExtensions: true }),
    ).toEqual(["--mode", "rpc", "--no-session", "--no-extensions"]);
  });

  it("isolates throwaway text jobs from tools and project context", () => {
    expect(buildPiSpawnArgs(PI_FLAVOR, { isolated: true })).toEqual([
      "--mode",
      "rpc",
      "--no-session",
      "--no-extensions",
      "--no-tools",
      "--no-skills",
      "--no-context-files",
    ]);
  });

  it("uses omp's renamed resume and context flags", () => {
    expect(buildPiSpawnArgs(OMP_FLAVOR, { resume: "abc123" })).toEqual([
      "--mode",
      "rpc",
      "--resume",
      "abc123",
    ]);
    expect(buildPiSpawnArgs(OMP_FLAVOR, { isolated: true })).toEqual([
      "--mode",
      "rpc",
      "--no-session",
      "--no-extensions",
      "--no-tools",
      "--no-skills",
      "--no-rules",
    ]);
  });
});

describe("parsePiModelRef", () => {
  it("splits provider/model ids", () => {
    expect(parsePiModelRef("anthropic/claude-sonnet-4-20250514")).toEqual({
      provider: "anthropic",
      modelId: "claude-sonnet-4-20250514",
    });
    expect(parsePiModelRef("")).toBeNull();
    expect(parsePiModelRef("sonnet")).toBeNull();
  });
});

describe("parsePiVersion", () => {
  it("reads a semver from CLI output", () => {
    expect(parsePiVersion("0.49.2")).toBe("0.49.2");
    expect(parsePiVersion("@earendil-works/pi-coding-agent/0.30.1")).toBe(
      "0.30.1",
    );
  });
});

describe("buildPiPrompt", () => {
  it("attaches vision images and steers while streaming", () => {
    const prompt = buildPiPrompt({
      text: "look",
      streaming: true,
      attachments: [
        {
          id: "a",
          name: "shot.png",
          mimeType: "image/png",
          kind: "image",
          size: 12,
          data: "abc",
        },
      ],
    });
    expect(prompt).toMatchObject({
      type: "prompt",
      message: "look",
      streamingBehavior: "steer",
      images: [{ type: "image", data: "abc", mimeType: "image/png" }],
    });
  });

  it("builds a steer command", () => {
    expect(buildPiSteer({ text: "stop" })).toEqual({
      type: "steer",
      message: "stop",
    });
  });
});

describe("RPC frames", () => {
  it("parses responses, events, and extension UI", () => {
    expect(parseJsonLine("not json")).toBeNull();
    const response = parseRpcResponse({
      type: "response",
      command: "prompt",
      success: true,
      id: "req-1",
    });
    expect(response).toEqual({
      id: "req-1",
      command: "prompt",
      success: true,
      data: undefined,
    });
    expect(
      parseRpcResponse({
        type: "response",
        command: "set_model",
        success: false,
        error: "missing",
      })?.error,
    ).toBe("missing");

    const confirm = parseExtensionUiRequest({
      type: "extension_ui_request",
      id: "ui-1",
      method: "confirm",
      title: "Dangerous",
      message: "Allow rm?",
    });
    expect(confirm).toEqual({
      id: "ui-1",
      method: "confirm",
      title: "Dangerous",
      message: "Allow rm?",
    });
    expect(needsExtensionUiReply(confirm!)).toBe(true);
    expect(extensionUiTitle(confirm!)).toBe("Dangerous — Allow rm?");
    expect(extensionUiResponse(confirm!, "allow")).toEqual({
      type: "extension_ui_response",
      id: "ui-1",
      confirmed: true,
    });
    expect(extensionUiResponse(confirm!, "deny")).toEqual({
      type: "extension_ui_response",
      id: "ui-1",
      cancelled: true,
    });

    const select = parseExtensionUiRequest({
      type: "extension_ui_request",
      id: "ui-2",
      method: "select",
      title: "Pick",
      options: ["a", "b"],
    });
    expect(extensionUiResponse(select!, "allow")).toEqual({
      type: "extension_ui_response",
      id: "ui-2",
      value: "a",
    });

    const notify = parseExtensionUiRequest({
      type: "extension_ui_request",
      id: "ui-3",
      method: "notify",
      message: "loaded",
    });
    expect(needsExtensionUiReply(notify!)).toBe(false);
  });
});

describe("streaming events", () => {
  it("maps text, thinking, tools, and turn completion", () => {
    expect(
      assistantDeltaFromEvent({
        type: "message_update",
        assistantMessageEvent: { type: "text_delta", delta: "Hello" },
      }),
    ).toEqual({ kind: "text", text: "Hello" });
    expect(
      assistantDeltaFromEvent({
        type: "message_update",
        assistantMessageEvent: { type: "thinking_delta", delta: "hmm" },
      }),
    ).toEqual({ kind: "thinking", text: "hmm" });
    expect(
      assistantDeltaFromEvent({
        type: "message_update",
        assistantMessageEvent: { type: "text_delta", delta: "\n\n" },
      }),
    ).toEqual({ kind: "text", text: "\n\n" });

    expect(
      toolCallStartFromEvent({
        type: "message_update",
        assistantMessageEvent: {
          type: "toolcall_start",
          contentIndex: 1,
          id: "call_1",
          toolName: "write",
        },
      }),
    ).toEqual({ id: "call_1", name: "write", index: 1 });

    expect(
      toolExecutionEndFromEvent({
        type: "tool_execution_end",
        toolCallId: "call_1",
        toolName: "bash",
        isError: false,
        result: { content: [{ type: "text", text: "ok" }] },
      }),
    ).toEqual({
      id: "call_1",
      name: "bash",
      detail: "ok",
      isError: false,
    });

    expect(isAgentSettled({ type: "agent_settled" })).toBe(true);
    expect(agentEndWillRetry({ type: "agent_end", willRetry: true })).toBe(true);
    expect(agentEndWillRetry({ type: "agent_end" })).toBe(false);
  });
});

describe("tools and models", () => {
  it("titles built-in Pi tools", () => {
    expect(toolKindFromName("bash")).toBe("execute");
    expect(toolKindFromName("edit")).toBe("edit");
    expect(toolTitle("read", { path: "src/a.ts" })).toMatch(/src\/a\.ts/);
    expect(previewFromTool("write", { path: "src/a.ts" })?.kind).toBe("write");
  });

  it("flattens get_available_models payloads", () => {
    const models = modelsFromRpcData(PI_FLAVOR, {
      models: [
        {
          id: "claude-sonnet-4-20250514",
          name: "Claude Sonnet 4",
          provider: "anthropic",
          reasoning: true,
          contextWindow: 200000,
        },
        {
          id: "gpt-4o",
          name: "GPT-4o",
          provider: "openai",
          reasoning: false,
        },
      ],
    });
    expect(models.map((model) => model.id)).toEqual([
      "pi:anthropic/claude-sonnet-4-20250514",
      "pi:openai/gpt-4o",
    ]);
    expect(models[0]?.settings?.[0]?.id).toBe("thinking");
    expect(models[0]?.contextWindow).toBe(200000);
    expect(models[1]?.settings).toBeUndefined();
  });

  it("reads session and context stats", () => {
    expect(
      providerSessionIdFromState({
        sessionId: "abc",
        sessionFile: "/tmp/session.jsonl",
        model: { contextWindow: 1000 },
      }),
    ).toBe("abc");
    expect(
      providerSessionIdFromState({
        sessionFile: "/tmp/session.jsonl",
      }),
    ).toBeUndefined();
    expect(
      contextFromUsage({
        usage: { totalTokens: 120 },
      }, 200),
    ).toEqual({ used: 120, window: 200 });
    // Live 0.80.x shapes: finished total on the assistant message, streaming
    // total on the nested partial. Tool-result usage is a nested LLM call, not
    // the context-window level.
    expect(
      contextFromUsage(
        {
          type: "message_end",
          message: { role: "assistant", usage: { totalTokens: 18014 } },
        },
        200000,
      ),
    ).toEqual({ used: 18014, window: 200000 });
    expect(
      contextFromUsage(
        {
          type: "message_update",
          assistantMessageEvent: {
            type: "text_delta",
            partial: { usage: { input: 3, output: 1, cacheWrite: 18007 } },
          },
        },
        200000,
      ),
    ).toEqual({ used: 18011, window: 200000 });
    expect(
      contextFromUsage(
        { type: "message_end", message: { role: "user" } },
        200000,
      ),
    ).toBeNull();
    expect(
      contextFromUsage(
        {
          type: "message_end",
          message: {
            role: "toolResult",
            usage: { totalTokens: 150 },
          },
        },
        200000,
      ),
    ).toBeNull();
    expect(
      contextFromSessionStats({
        contextUsage: { tokens: 60, contextWindow: 200000, percent: 30 },
      }),
    ).toEqual({ used: 60, window: 200000 });
  });
});

describe("turnErrorFromEvent", () => {
  it("reads the reason a turn failed with no content", () => {
    expect(
      turnErrorFromEvent({
        type: "message_end",
        message: {
          role: "assistant",
          content: [],
          stopReason: "error",
          errorMessage: "No API key for provider: openai-codex",
        },
      }),
    ).toBe("No API key for provider: openai-codex");
  });

  it("still reports a failure that carries no reason", () => {
    expect(
      turnErrorFromEvent({
        type: "message_end",
        message: { role: "assistant", stopReason: "error" },
      }),
    ).toBe("");
  });

  it("ignores healthy messages, other roles, and other frames", () => {
    expect(
      turnErrorFromEvent({
        type: "message_end",
        message: { role: "assistant", stopReason: "end_turn", content: [{ type: "text", text: "hi" }] },
      }),
    ).toBeNull();
    expect(
      turnErrorFromEvent({
        type: "message_end",
        message: {
          role: "toolResult",
          stopReason: "error",
          errorMessage: "tool failed",
        },
      }),
    ).toBeNull();
    expect(
      turnErrorFromEvent({
        type: "turn_end",
        message: {
          role: "assistant",
          stopReason: "error",
          errorMessage: "boom",
        },
      }),
    ).toBeNull();
  });
});
