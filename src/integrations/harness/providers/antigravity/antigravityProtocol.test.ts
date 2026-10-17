import { describe, expect, it } from "vitest";
import { harnessSupportsAttachments, RUNTIME_MODES } from "../../../../features/sessions/model/session";
import { ATTACHMENT_ONLY_PROMPT } from "../../../../features/sessions/model/attachments";
import * as antigravity from "./antigravityProtocol";

const providers = [
  { id: "antigravity", protocol: antigravity, mode: antigravity.antigravityModeId,
    blocks: antigravity.antigravityPromptBlocks,
    modes: ["default", "auto_edit", "default", "yolo"], plan: "default" },
] as const;

describe.each(providers)("$id ACP protocol", ({ id, protocol, mode, blocks, modes, plan }) => {
  it("maps every runtime mode and overrides full access for planning", () => {
    expect(RUNTIME_MODES.map((runtimeMode) => mode(runtimeMode))).toEqual(modes);
    for (const runtimeMode of RUNTIME_MODES) expect(mode(runtimeMode, true)).toBe(plan);
  });

  it("delivers image-only prompts and leaves attachments enabled", () => {
    const image = {
      id: "image", name: "image.png", kind: "image", mimeType: "image/png",
      size: 4, data: "aGV5",
    } as const;
    expect(harnessSupportsAttachments(id)).toBe(true);
    expect(blocks("", [image])).toEqual([
      { type: "text", text: ATTACHMENT_ONLY_PROMPT },
      { type: "image", mimeType: "image/png", data: "aGV5" },
    ]);
    expect(blocks(" hi ")).toEqual([{ type: "text", text: "hi" }]);
    expect(blocks("  ")).toEqual([]);
  });

  it("extracts session IDs and rejects malformed IDs", () => {
    for (const key of ["sessionId", "session_id", "id"]) {
      expect(protocol.sessionIdFromResult({ [key]: " S1 " })).toBe("S1");
    }
    for (const raw of [null, {}, { sessionId: " " }, { sessionId: 42 }]) {
      expect(protocol.sessionIdFromResult(raw)).toBeUndefined();
    }
  });

  it("resolves config IDs by category without selecting the provider", () => {
    const options = protocol.readConfigOptions([
      null, {}, { id: "provider", category: "model" },
      { id: "model_picker", category: "model", currentValue: "m1" },
      { id: "thinking", category: "thought_level", currentValue: "high" },
    ]);
    expect(options).toHaveLength(3);
    expect(protocol.extractModelConfigId(options)).toBe("model_picker");
    expect(protocol.extractModelConfigId([])).toBe("model");
    expect(protocol.resolveSettingConfigId(options, "effort")).toBe("thinking");
    expect(protocol.resolveSettingConfigId(options, "reasoning")).toBe("thinking");
    expect(protocol.resolveSettingConfigId(options, "THINKING")).toBe("thinking");
    expect(protocol.resolveSettingConfigId(options, "missing")).toBeUndefined();
  });

  it("discovers live models and thinking levels, including grouped choices", () => {
    const models = protocol.modelsFromSessionNew({ configOptions: [
      { id: "model", category: "model", options: [
        { group: "provider", options: [
          { value: "m1", name: "Model One" }, { value: "m2", name: "Model Two" },
        ] },
      ] },
      { id: "thinking", category: "thought_level", currentValue: "high", options: [
        { value: "low", name: "Low" }, { value: "high", name: "High" },
        { value: "max", name: "Max" },
      ] },
    ] });
    expect(models.map((model) => model.id)).toEqual([`${id}:m1`, `${id}:m2`]);
    expect(models[0]).toMatchObject({ harness: id, name: "Model One", nativeId: "m1",
      settings: [{ id: "effort", value: "high", options: [
        { value: "low", label: "Low" }, { value: "high", label: "High" },
        { value: "max", label: "Max" },
      ] }],
    });
    expect(protocol.modelsFromSessionNew(null)).toEqual([]);
  });

  it("falls back to top-level ACP models without inventing reasoning controls", () => {
    expect(protocol.modelsFromSessionNew({ models: { availableModels: [
      { modelId: "gemini-pro-agent", name: "Gemini 3.1 Pro (High)" },
    ] } })).toEqual([{
      id: `${id}:gemini-pro-agent`, harness: id, nativeId: "gemini-pro-agent",
      name: "Gemini 3.1 Pro (High)",
    }]);
  });

  it("uses opaque permission IDs by semantic kind and never fabricates an ID", () => {
    const request = protocol.permissionRequestFromAcp({
      toolCall: { toolCallId: "t1", title: "Write file", kind: "edit" },
      options: [
        { optionId: "yes-7", kind: "allow_once" },
        { optionId: "no-9", kind: "reject_once" },
      ],
    });
    expect(request).toMatchObject({ callId: "t1", kind: "edit" });
    expect(protocol.permissionOptionId("allow", request.optionIds, request.optionKinds)).toBe("yes-7");
    expect(protocol.permissionOptionId("deny", request.optionIds, request.optionKinds)).toBe("no-9");
    expect(protocol.permissionOptionId("deny", ["allow_once"])).toBeNull();
    expect(protocol.permissionOptionId("allow", [])).toBeNull();
    expect(protocol.autoPermissionOption("supervised", "edit", request.optionIds, request.optionKinds)).toBeNull();
    expect(protocol.autoPermissionOption("auto", "execute", request.optionIds, request.optionKinds)).toBeNull();
    expect(protocol.autoPermissionOption("auto-accept-edits", "execute", request.optionIds, request.optionKinds)).toBeNull();
    expect(protocol.autoPermissionOption("auto-accept-edits", "edit", request.optionIds, request.optionKinds)).toBe("yes-7");
    expect(protocol.autoPermissionOption("full-access", "execute", request.optionIds, request.optionKinds)).toBe("yes-7");
  });

  it("maps text, reasoning, tools and plans without fx-specific result decoding", () => {
    const parse = (update: unknown) => protocol.eventsFromAcpUpdate({ update });
    expect(parse({ sessionUpdate: "agent_message_chunk", content: { type: "text", text: "hi" } }))
      .toEqual([{ type: "message.delta", text: "hi" }]);
    expect(parse({ sessionUpdate: "agent_thought_chunk", content: { text: "think" } }))
      .toEqual([{ type: "reasoning.delta", text: "think" }]);
    expect(parse({ sessionUpdate: "tool_call", toolCallId: "t", title: "Read file", kind: "read", status: "completed" }))
      .toMatchObject([{ type: "tool.updated", callId: "t", kind: "read", status: "completed" }]);
    expect(parse({ sessionUpdate: "plan", entries: [{ content: "Check code", status: "pending" }] }))
      .toMatchObject([{ type: "tasks.updated", items: [{ text: "Check code" }] }]);
    expect(parse({ sessionUpdate: "available_commands_update", availableCommands: [] })).toEqual([]);
  });

  it("preserves structured command output as JSON for the compact command renderer", () => {
    const commands = ["pwd", "git status", "git branch --show-current", "git diff --stat", "npm test"];
    const results = commands.map((command, index) => ({
      command, stdout: `output ${index + 1}`, stderr: "", exitCode: 0,
    }));
    const events = protocol.eventsFromAcpUpdate({ sessionId: "session-one", update: {
      sessionUpdate: "tool_call_update", toolCallId: "commands-5",
      title: "Running 5 commands", kind: "execute", status: "completed",
      rawInput: { commands }, rawOutput: { results },
    } });
    expect(events).toHaveLength(1);
    expect(events[0]).toMatchObject({
      type: "tool.updated", callId: "commands-5", kind: "execute", status: "completed",
      preview: { kind: "shell" },
    });
    if (events[0]?.type !== "tool.updated") throw new Error("Expected tool output");
    expect(JSON.parse(events[0].detail!)).toEqual({ results });
    expect(events[0].preview?.output).toBe(events[0].detail);
  });

  it("decodes ACP content blocks and read results with explicit output provenance", () => {
    const content = [{ type: "content", content: { type: "text", text: "first line\nsecond line" } },
      { type: "diff", path: "/repo/a.ts", newText: "not a command result" },
      { type: "terminal", terminalId: "terminal-1" },
      { type: "content", content: { type: "resource", resource: { uri: "file:///repo/a.ts", text: "third line" } } }];
    const [read] = protocol.eventsFromAcpUpdate({ update: {
      sessionUpdate: "tool_call_update", toolCallId: "read-1", kind: "read", title: "Read a.ts",
      status: "in_progress", rawInput: { path: "/repo/a.ts", text: "input sentinel" },
      content,
    } });
    expect(read).toMatchObject({ type: "tool.updated", callId: "read-1", status: "in_progress",
      detail: "first line\nsecond line\nthird line", preview: { kind: "read", path: "/repo/a.ts",
        output: "first line\nsecond line\nthird line" } });
    const [finished] = protocol.eventsFromAcpUpdate({ update: {
      sessionUpdate: "tool_call_update", toolCallId: "read-1", status: "completed",
    } });
    // A later status-only patch must re-emit the previously received output
    // so the transcript marks it as a successful result, not request input.
    expect(finished).toMatchObject({ type: "tool.updated", callId: "read-1", status: "completed",
      detail: "first line\nsecond line\nthird line", preview: { kind: "read",
        output: "first line\nsecond line\nthird line" } });
  });

  it("supports alternate explicit result/output fields and preserves falsy outputs", () => {
    const parse = (fields: Record<string, unknown>) => protocol.eventsFromAcpUpdate({ update: {
      sessionUpdate: "tool_call_update", toolCallId: "one", status: "completed", ...fields,
    } })[0];
    expect(parse({ rawOutput: { stdout: "ok", stderr: "", exitCode: 0 } }))
      .toMatchObject({ detail: '{\n  "stdout": "ok",\n  "stderr": "",\n  "exitCode": 0\n}' });
    expect(parse({ raw_output: ["a", "b"] })).toMatchObject({ detail: '[\n  "a",\n  "b"\n]' });
    expect(parse({ output: { content: [{ type: "text", text: "tool output" }] } }))
      .toMatchObject({ detail: "tool output" });
    expect(parse({ result: { output: "nested result" } }))
      .toMatchObject({ detail: "nested result" });
    expect(parse({ rawOutput: 0 })).toMatchObject({ detail: "0" });
    expect(parse({ rawOutput: false })).toMatchObject({ detail: "false" });
  });

  it("never relabels request input, diffs, or terminal references as successful output", () => {
    const [inputOnly] = protocol.eventsFromAcpUpdate({ update: {
      sessionUpdate: "tool_call", toolCallId: "read-only", kind: "read",
      title: "Read a.ts", status: "completed",
      rawInput: { path: "/repo/a.ts", content: "request argument" },
      content: [{ type: "diff", path: "/repo/a.ts", newText: "new code" },
        { type: "terminal", terminalId: "term-1" }],
    } });
    expect(inputOnly).toMatchObject({ type: "tool.updated", callId: "read-only", status: "completed" });
    if (inputOnly.type !== "tool.updated") throw new Error("Expected tool update");
    expect(inputOnly.detail).toBeUndefined();
    expect(inputOnly.preview?.output).toBeUndefined();
    const [chunk] = protocol.eventsFromAcpUpdate({ update: {
      sessionUpdate: "tool_call_content_chunk", toolCallId: "read-chunk", kind: "read",
      content: { type: "content", content: { type: "text", text: "read output" } },
    } });
    expect(chunk).toMatchObject({ type: "tool.updated", callId: "read-chunk",
      detail: "read output", preview: { kind: "read", output: "read output" } });
  });

  it("retains output and its shell/read provenance across ACP status-only patches", () => {
    for (const [callId, kind] of [["commands-5-patches", "execute"], ["read-patches", "read"]] as const) {
      const notification = (update: Record<string, unknown>) => protocol.eventsFromAcpUpdate({
        sessionId: "patch-test-session", update: { toolCallId: callId, ...update },
      })[0];
      const started = notification({ sessionUpdate: "tool_call", kind, title: kind === "read" ? "Read a.ts" : "Running 5 commands",
        rawInput: { command: "do something" } });
      expect(started).toMatchObject({ type: "tool.updated", callId });
      const partial = notification({ sessionUpdate: "tool_call_update", status: "in_progress",
        rawOutput: { stdout: "real result", exitCode: 0 } });
      const output = kind === "execute"
        ? '{"stdout":"real result","exitCode":0}'
        : '{\n  "stdout": "real result",\n  "exitCode": 0\n}';
      expect(partial).toMatchObject({ type: "tool.updated", preview: {
        kind: kind === "execute" ? "shell" : "read",
        output,
      } });
      const complete = notification({ sessionUpdate: "tool_call_update", status: "completed" });
      expect(complete).toMatchObject({ type: "tool.updated", status: "completed",
        title: started.type === "tool.updated" ? started.title : undefined,
        detail: output,
        preview: { kind: kind === "execute" ? "shell" : "read" } });
    }
  });

  it("retains previously reported output on a status-only cancellation before discarding cache", () => {
    const notification = (update: Record<string, unknown>) => protocol.eventsFromAcpUpdate({
      sessionId: "cancelled-output-regression", update: {
        sessionUpdate: "tool_call_update", toolCallId: "cancelled-shell", ...update,
      },
    })[0];
    notification({ kind: "execute", status: "in_progress",
      rawInput: { command: "INPUT_SENTINEL_NOT_A_RESULT" },
      rawOutput: { commandLine: "npm test", exitCode: 1,
        combinedOutput: "2 tests failed\n" },
    });
    const expectedOutput = JSON.stringify({ commandLine: "npm test", exitCode: 1,
      combinedOutput: "2 tests failed\n" });
    expect(notification({ status: "cancelled" })).toMatchObject({
      type: "tool.updated", callId: "cancelled-shell", kind: "execute",
      status: "cancelled", detail: expectedOutput,
      preview: { kind: "shell", output: expectedOutput },
    });
    // A settled cancellation must release its cached output, not replay it on
    // later updates reusing the same ID or promote the original request input.
    const recycled = notification({ status: "cancelled",
      rawInput: { command: "INPUT_SENTINEL_NOT_A_RESULT" } });
    if (recycled.type !== "tool.updated") throw new Error("Expected tool update");
    expect(recycled.detail).toBeUndefined();
    expect(recycled.preview?.output).toBeUndefined();
  });

  it("appends ACP tool_call_content_chunk output without joining it to input", () => {
    const notification = (update: Record<string, unknown>) => protocol.eventsFromAcpUpdate({
      sessionId: "chunk-test-session", update: { toolCallId: "read-stream", ...update },
    })[0];
    notification({ sessionUpdate: "tool_call", kind: "read", title: "Read file",
      rawInput: { path: "/repo/file.ts", text: "should not appear" } });
    const a = notification({ sessionUpdate: "tool_call_content_chunk",
      content: { type: "content", content: { type: "text", text: "line one" } } });
    const b = notification({ sessionUpdate: "tool_call_content_chunk",
      content: { type: "content", content: { type: "text", text: "line two" } } });
    expect(a).toMatchObject({ detail: "line one", preview: { output: "line one" } });
    expect(b).toMatchObject({ detail: "line one\nline two", preview: { kind: "read", output: "line one\nline two" } });
    expect(notification({ sessionUpdate: "tool_call_update", status: "completed" }))
      .toMatchObject({ status: "completed", detail: "line one\nline two",
        preview: { kind: "read", output: "line one\nline two" } });
  });

  it("normalizes nested and stringified ACP command results for the renderer", () => {
    const parse = (rawOutput: unknown, callId: string) => protocol.eventsFromAcpUpdate({
      sessionId: "structured-command-output", update: {
        sessionUpdate: "tool_call_update", toolCallId: callId,
        title: "Running 5 commands", kind: "execute", status: "completed",
        rawInput: { command: "INPUT MUST NOT BECOME OUTPUT" }, rawOutput,
      },
    })[0];
    const grouped = { results: [
      { commandLine: "pwd", workingDir: "/repo", exitCode: 0,
        combinedOutput: "/repo\n", stdout: "/repo\n", stderr: "" },
      { commandLine: "npm test", workingDir: "/repo", exitCode: 1,
        combinedOutput: "1 failed\n", stdout: "1 failed\n", stderr: "1 failed\n" },
    ] };
    for (const [index, raw] of [grouped, JSON.stringify(grouped),
      { result: JSON.stringify(grouped) }, { output: grouped.results }].entries()) {
      const event = parse(raw, `commands-${index}`);
      expect(event).toMatchObject({ type: "tool.updated", status: "completed",
        preview: { kind: "shell" } });
      if (event.type !== "tool.updated") throw new Error("Expected tool output");
      expect(JSON.parse(event.detail!)).toEqual(grouped);
      expect(event.preview?.output).toBe(event.detail);
      expect(event.detail).not.toContain("INPUT MUST NOT BECOME OUTPUT");
    }
  });

  it("retains each explicit output field so the renderer can select one stream without duplicates", () => {
    const parse = (rawOutput: unknown, id: string) => protocol.eventsFromAcpUpdate({
      update: { sessionUpdate: "tool_call_update", toolCallId: id, kind: "execute",
        status: "completed", rawOutput },
    })[0];
    expect(parse({ commandLine: "echo done", exitCode: 0,
      combinedOutput: "done\n", stdout: "done\n", stderr: "done\n" }, "combined"))
      .toMatchObject({ detail: '{"commandLine":"echo done","exitCode":0,"combinedOutput":"done\\n","stdout":"done\\n","stderr":"done\\n"}' });
    expect(parse({ stdout: "one\ntwo\n", stderr: "one\ntwo\n", exitCode: 0 }, "duplicate"))
      .toMatchObject({ detail: '{"stdout":"one\\ntwo\\n","stderr":"one\\ntwo\\n","exitCode":0}' });
    expect(parse({ stdout: "out\n", stderr: "error\n", exitCode: 1 }, "distinct"))
      .toMatchObject({ detail: '{"stdout":"out\\n","stderr":"error\\n","exitCode":1}' });
  });

  it("truncates long command output within JSON fields so the envelope stays valid", () => {
    const body = "result-line\n".repeat(1_000);
    const [event] = protocol.eventsFromAcpUpdate({ update: {
      sessionUpdate: "tool_call_update", toolCallId: "large-commands", kind: "execute",
      status: "completed", rawOutput: JSON.stringify({ commandLine: "npm test",
        workingDir: "/repo", exitCode: 0, combinedOutput: body, stdout: body }),
    } });
    expect(event).toMatchObject({ type: "tool.updated", status: "completed",
      preview: { kind: "shell" } });
    if (event.type !== "tool.updated") throw new Error("Expected tool output");
    expect(event.detail?.length).toBeLessThanOrEqual(8_000);
    const json = JSON.parse(event.detail!) as Record<string, unknown>;
    expect(json.commandLine).toBe("npm test");
    expect(json.workingDir).toBe("/repo");
    expect(json.exitCode).toBe(0);
    expect(String(json.combinedOutput).startsWith("result-line\n")).toBe(true);
    expect(String(json.combinedOutput).endsWith("\n…")).toBe(true);
    expect(event.preview?.output).toBe(event.detail);
  });

  it("preserves command metadata and the preferred combined output in capped grouped results", () => {
    const log = "Tests failed\n" + "failure detail\n".repeat(1_000);
    const commands = { results: [
      { commandLine: "pwd", workingDir: "/repo", exitCode: 0,
        combinedOutput: "/repo\n", stdout: "/repo\n", stderr: "" },
      { commandLine: "npm test", workingDir: "/repo", exitCode: 1,
        combinedOutput: log, stdout: log, stderr: "failure detail\n" },
    ] };
    const [event] = protocol.eventsFromAcpUpdate({ sessionId: "big-group", update: {
      sessionUpdate: "tool_call_update", toolCallId: "big-group-tools", kind: "execute",
      status: "completed", rawOutput: { result: JSON.stringify(commands) },
    } });
    if (event.type !== "tool.updated") throw new Error("Expected tool output");
    expect(event.detail!.length).toBeLessThanOrEqual(8_000);
    const result = JSON.parse(event.detail!) as typeof commands;
    expect(result.results[0]).toEqual(commands.results[0]);
    expect(result.results[1]).toMatchObject({ commandLine: "npm test", workingDir: "/repo", exitCode: 1 });
    expect(result.results[1].combinedOutput.startsWith("Tests failed\n")).toBe(true);
    expect(result.results[1].combinedOutput.endsWith("\n…")).toBe(true);
    expect(event.preview?.output).toBe(event.detail);
  });

  it("does not reinterpret read-file JSON text as a command or promote request-only fields", () => {
    const file = JSON.stringify({ commandLine: "literal read-file content", combinedOutput: "value" });
    const [read] = protocol.eventsFromAcpUpdate({ update: {
      sessionUpdate: "tool_call_update", toolCallId: "read-json", kind: "read", status: "completed",
      rawInput: { path: "/repo/fixture.json", content: "request-only" }, rawOutput: file,
    } });
    expect(read).toMatchObject({ type: "tool.updated", detail: file });
    const [inputOnly] = protocol.eventsFromAcpUpdate({ update: {
      sessionUpdate: "tool_call_update", toolCallId: "shell-input-only", kind: "execute", status: "completed",
      rawInput: { commandLine: "should stay hidden", combinedOutput: "request content" },
    } });
    expect(inputOnly.type === "tool.updated" ? inputOnly.detail : null).toBeUndefined();
  });
});
