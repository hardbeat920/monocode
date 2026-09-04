import { describe, expect, it } from "vitest";
import {
  autoPermissionOption,
  eventsFromAcpUpdate,
  extractModelConfigId,
  fxModeId,
  fxPromptBlocks,
  mergeFxCatalogModels,
  modelFromFxStatusOutput,
  modelsFromFxOutput,
  permissionOptionId,
  permissionRequestFromAcp,
  readConfigOptions,
  sessionIdFromResult,
} from "./fxProtocol";
import { harnessSupportsAttachments } from "../session";

describe("fx protocol", () => {
  // fx's "ask" mode stops for every read and command, and we surface none of
  // those prompts, so a turn would park forever. Always use fx's auto mode.
  it("always runs fx in its own code (auto) mode", () => {
    expect(fxModeId("supervised")).toBe("code");
    expect(fxModeId("auto-accept-edits")).toBe("code");
    expect(fxModeId("auto")).toBe("code");
    expect(fxModeId("full-access")).toBe("code");
  });

  it("sends text-only prompt blocks", () => {
    expect(fxPromptBlocks("  hello  ")).toEqual([
      { type: "text", text: "hello" },
    ]);
    expect(fxPromptBlocks("   ")).toEqual([]);
  });

  it("does not support attachments", () => {
    expect(harnessSupportsAttachments("fx")).toBe(false);
    expect(harnessSupportsAttachments("grok")).toBe(true);
    expect(harnessSupportsAttachments("cursor")).toBe(true);
  });

  it("auto-allows in every runtime mode so a turn never parks on approval", () => {
    const options = ["allow-once", "reject-once"];
    expect(autoPermissionOption("supervised", options)).toBe("allow-once");
    expect(autoPermissionOption("auto", options)).toBe("allow-once");
    expect(autoPermissionOption("full-access", options)).toBe("allow-once");
    expect(autoPermissionOption("supervised", [])).toBeNull();
  });

  it("prefers allow_always so fx stops re-asking", () => {
    expect(
      autoPermissionOption("supervised", [
        "allow_once",
        "allow_always",
        "reject_once",
      ]),
    ).toBe("allow_always");
  });

  it("picks allow/reject option ids from ACP permission options", () => {
    expect(
      permissionOptionId("allow", ["allow_once", "reject_once"]),
    ).toBe("allow_once");
    expect(
      permissionOptionId("deny", ["allow-once", "reject-once"]),
    ).toBe("reject-once");
  });

  it("reads a permission prompt from an ACP request", () => {
    const request = permissionRequestFromAcp({
      toolCall: {
        toolCallId: "call-1",
        kind: "edit",
        title: "Edit src/lib/fx.ts",
      },
      options: [{ optionId: "allow-once" }, { optionId: "reject-once" }],
    });
    expect(request.callId).toBe("call-1");
    expect(request.kind).toBe("edit");
    expect(request.optionIds).toEqual(["allow-once", "reject-once"]);
    expect(request.title).toContain("fx.ts");
  });

  it("extracts nested shell args from permission payloads", () => {
    const request = permissionRequestFromAcp({
      toolCall: {
        toolCallId: "call_a",
        title: "terminal.exec git status -s",
        kind: "execute",
        rawInput: {
          action: "exec",
          command: "git status -s",
          cwd: "/repo",
        },
      },
      options: [{ optionId: "allow_once" }],
    });
    expect(request.callId).toBe("call_a");
    expect(request.title).toContain("git status -s");
  });

  it("maps agent message and tool updates to harness events", () => {
    expect(
      eventsFromAcpUpdate({
        sessionUpdate: "agent_message_chunk",
        content: { type: "text", text: "Hi" },
      }),
    ).toEqual([{ type: "message.delta", text: "Hi" }]);

    const tools = eventsFromAcpUpdate({
      sessionUpdate: "tool_call",
      toolCallId: "t1",
      kind: "read",
      title: "Read README.md",
      status: "in_progress",
    });
    expect(tools[0]).toMatchObject({
      type: "tool.updated",
      callId: "t1",
      kind: "read",
      status: "in_progress",
    });
  });

  it("maps plan entries and usage", () => {
    expect(
      eventsFromAcpUpdate({
        sessionUpdate: "plan",
        entries: [
          { content: "Inspect router", status: "completed" },
          { content: "Add test", status: "pending" },
        ],
      }),
    ).toEqual([
      {
        type: "tasks.updated",
        items: [
          { text: "Inspect router", status: "completed" },
          { text: "Add test", status: "pending" },
        ],
      },
    ]);
    expect(
      eventsFromAcpUpdate({ sessionUpdate: "plan", text: "# Approach" }),
    ).toEqual([{ type: "plan", text: "# Approach" }]);

    expect(
      eventsFromAcpUpdate({
        usage: { used: 1200, window: 200000 },
      }),
    ).toEqual([{ type: "context", used: 1200, window: 200000 }]);
  });

  it("parses fx models --json", () => {
    const models = modelsFromFxOutput(
      JSON.stringify({
        kind: "models",
        count: 2,
        ids: ["zai/glm-5.2-fast", "openai/gpt-5.2"],
      }),
    );
    expect(models).toEqual([
      {
        id: "fx:zai/glm-5.2-fast",
        harness: "fx",
        name: "zai/glm-5.2-fast",
        nativeId: "zai/glm-5.2-fast",
      },
      {
        id: "fx:openai/gpt-5.2",
        harness: "fx",
        name: "openai/gpt-5.2",
        nativeId: "openai/gpt-5.2",
      },
    ]);
  });

  it("parses object-shaped model entries when present", () => {
    const models = modelsFromFxOutput(
      JSON.stringify({
        models: [
          {
            id: "zai/glm-5.2-fast",
            name: "GLM 5.2 Fast",
            contextWindow: 202752,
          },
        ],
      }),
    );
    expect(models).toEqual([
      {
        id: "fx:zai/glm-5.2-fast",
        harness: "fx",
        name: "GLM 5.2 Fast",
        nativeId: "zai/glm-5.2-fast",
        contextWindow: 202752,
      },
    ]);
  });

  it("parses a text model list when json is missing", () => {
    const models = modelsFromFxOutput(
      "zai/glm-5.2-fast - GLM 5.2 Fast (default)\nopenai/gpt-5.4 - GPT-5.4\n",
    );
    expect(models.map((model) => model.nativeId)).toEqual([
      "zai/glm-5.2-fast",
      "openai/gpt-5.4",
    ]);
  });

  it("adds the TUI-selected status model when the list command omits it", () => {
    const listed = modelsFromFxOutput(
      JSON.stringify({ ids: ["zai/glm-4.7", "openai/gpt-5.2"] }),
    );
    const active = modelFromFxStatusOutput(
      JSON.stringify({ kind: "status", model: "zai/glm-5.2" }),
    );
    expect(
      mergeFxCatalogModels(listed, active).map((model) => model.nativeId),
    ).toEqual(["zai/glm-4.7", "openai/gpt-5.2", "zai/glm-5.2"]);
  });

  it("reads a session id from ACP setup results", () => {
    expect(sessionIdFromResult({ sessionId: "  abc  " })).toBe("abc");
    expect(sessionIdFromResult({ session_id: "xyz" })).toBe("xyz");
    expect(sessionIdFromResult({})).toBeUndefined();
  });

  it("picks the model config option, not the provider listed first", () => {
    const options = readConfigOptions([
      {
        id: "provider",
        name: "Provider",
        category: "model",
        type: "select",
        currentValue: "gateway",
      },
      {
        id: "model",
        name: "Model",
        category: "model",
        type: "select",
        currentValue: "zai/glm-5.2",
      },
      {
        id: "mode",
        name: "Session Mode",
        category: "mode",
        type: "select",
        currentValue: "ask",
      },
    ]);
    expect(extractModelConfigId(options)).toBe("model");
  });

  // Payloads below are verbatim from an `fx acp` 0.0.5 wire capture.
  it("recovers a read target from the fx result blob", () => {
    const [event] = eventsFromAcpUpdate({
      update: {
        sessionUpdate: "tool_call_update",
        toolCallId: "call_1",
        status: "completed",
        content: [
          {
            type: "content",
            content: {
              type: "text",
              text: "<path>notes.txt</path>\n<content>\n1\thello\n2\tworld\n</content>",
            },
          },
        ],
      },
    });
    expect(event).toMatchObject({
      type: "tool.updated",
      title: "Read notes.txt",
      preview: { kind: "read", path: "notes.txt", fileName: "notes.txt" },
    });
  });

  it("never slices a gerund title into 'Read ing'", () => {
    const [event] = eventsFromAcpUpdate({
      update: {
        sessionUpdate: "tool_call",
        toolCallId: "call_2",
        title: "Reading",
        kind: "read",
        status: "pending",
      },
    });
    expect(event).toMatchObject({ type: "tool.updated", title: "Read" });
  });

  it("recovers the grep query", () => {
    const [event] = eventsFromAcpUpdate({
      update: {
        sessionUpdate: "tool_call_update",
        toolCallId: "call_3",
        status: "completed",
        content: [
          {
            type: "content",
            content: {
              type: "text",
              text: "[grep] 2 matches for export\n - app.ts:1: export const a = 1;\n",
            },
          },
        ],
      },
    });
    expect(event).toMatchObject({
      title: "Find export",
      preview: { kind: "search", query: "export" },
    });
  });

  it("labels a shell call with the command from fx's command_result", () => {
    const [event] = eventsFromAcpUpdate({
      update: {
        sessionUpdate: "tool_call_update",
        toolCallId: "call_4",
        kind: "execute",
        status: "completed",
        content: [
          {
            type: "content",
            content: {
              type: "text",
              text: "exit_code=0\n<stdout>\nhi\n</stdout>\n",
            },
          },
        ],
        command_result: {
          kind: "foreground",
          command: "echo hi",
          cwd: "/tmp/x",
          exit_code: 0,
        },
      },
    });
    expect(event).toMatchObject({ title: "echo hi", detail: "hi" });
  });

  it("keeps stderr and the exit code for a failed shell call", () => {
    const [event] = eventsFromAcpUpdate({
      update: {
        sessionUpdate: "tool_call_update",
        toolCallId: "call_failed",
        kind: "execute",
        status: "failed",
        content: [
          {
            type: "content",
            content: {
              type: "text",
              text: "exit_code=1\n<stdout>\n</stdout>\n<stderr>\ncommand failed\n</stderr>\n",
            },
          },
        ],
        command_result: {
          kind: "foreground",
          command: "false",
          cwd: "/tmp/x",
          exit_code: 1,
        },
      },
    });
    expect(event).toMatchObject({
      title: "false",
      detail: "command failed\nexit 1",
    });
  });

  it("recovers the edited path from fx's write confirmation", () => {
    const [event] = eventsFromAcpUpdate({
      update: {
        sessionUpdate: "tool_call_update",
        toolCallId: "call_5",
        kind: "edit",
        status: "completed",
        content: [
          {
            type: "content",
            content: { type: "text", text: "edited app.ts (41 bytes)" },
          },
        ],
      },
    });
    expect(event).toMatchObject({
      title: "Edit app.ts",
      preview: { kind: "write", path: "app.ts", fileName: "app.ts" },
    });
  });
});
