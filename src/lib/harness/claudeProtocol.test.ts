import { describe, expect, it } from "vitest";
import { modelsForClaudeVersion } from "./claudeCatalog";
import {
  applyClaudeCompletedText,
  applyClaudePromptEffortPrefix,
  applyClaudeTextDelta,
  askUserQuestionAllowInput,
  buildClaudeSpawnArgs,
  buildClaudeUserMessage,
  completedAssistantParts,
  createClaudeTextState,
  contextFromResult,
  contextUsedFromAssistant,
  extractExitPlanModePlan,
  isTodoTool,
  markClaudeContentPublished,
  normalizeClaudeCliEffort,
  parseClaudeVersion,
  parseControlRequest,
  planTextFromTodos,
  resolveClaudeApiModelId,
  reconcileClaudeCompletedTool,
  runtimeModeToPermission,
  statusTextFromSystem,
  streamDeltaFromEvent,
  toClaudePermissionResult,
  toolKindFromName,
  toolStartFromEvent,
  turnStatusFromResult,
} from "./claudeProtocol";

describe("runtimeModeToPermission", () => {
  it("maps runtime modes onto Claude permission flags", () => {
    expect(runtimeModeToPermission("supervised")).toBeUndefined();
    expect(runtimeModeToPermission("auto-accept-edits")).toBe("acceptEdits");
    expect(runtimeModeToPermission("auto")).toBe("auto");
    expect(runtimeModeToPermission("full-access")).toBe("bypassPermissions");
  });
});

describe("normalizeClaudeCliEffort", () => {
  it("drops ultrathink and maps ultracode to xhigh", () => {
    expect(
      normalizeClaudeCliEffort("ultrathink", "claude-sonnet-5"),
    ).toBeUndefined();
    expect(normalizeClaudeCliEffort("ultracode", "claude-opus-5")).toBe(
      "xhigh",
    );
  });

  it("maps xhigh to max on older models", () => {
    expect(normalizeClaudeCliEffort("xhigh", "claude-opus-4-6")).toBe("max");
    expect(normalizeClaudeCliEffort("xhigh", "claude-opus-5")).toBe("xhigh");
  });

  it("maps max to high on sonnet 4.6", () => {
    expect(normalizeClaudeCliEffort("max", "claude-sonnet-4-6")).toBe("high");
  });
});

describe("applyClaudePromptEffortPrefix", () => {
  it("prefixes ultrathink on the prompt", () => {
    expect(
      applyClaudePromptEffortPrefix("Investigate the edge cases", "ultrathink"),
    ).toBe("Ultrathink:\nInvestigate the edge cases");
    expect(applyClaudePromptEffortPrefix("hello", "high")).toBe("hello");
  });
});

describe("resolveClaudeApiModelId", () => {
  it("appends [1m] for the 1M context window", () => {
    expect(resolveClaudeApiModelId("claude-opus-5", "1m")).toBe(
      "claude-opus-5[1m]",
    );
    expect(resolveClaudeApiModelId("claude-sonnet-5", "200k")).toBe(
      "claude-sonnet-5",
    );
  });
});

describe("buildClaudeSpawnArgs", () => {
  it("speaks stream-json with stdio permissions like the Agent SDK", () => {
    const args = buildClaudeSpawnArgs({
      model: "claude-sonnet-5",
      effort: "high",
      permissionMode: "acceptEdits",
      sessionId: "sess-1",
    });
    expect(args).toContain("--output-format");
    expect(args).toContain("stream-json");
    expect(args).toContain("--input-format");
    expect(args).toContain("--permission-prompt-tool");
    expect(args).toContain("stdio");
    expect(args).toContain("--include-partial-messages");
    expect(args).toContain("--setting-sources=user,project,local");
    expect(args).toEqual(
      expect.arrayContaining([
        "--model",
        "claude-sonnet-5",
        "--effort",
        "high",
      ]),
    );
    expect(args).toEqual(
      expect.arrayContaining(["--permission-mode", "acceptEdits"]),
    );
    expect(args).toEqual(expect.arrayContaining(["--session-id", "sess-1"]));
    const settings = args[args.indexOf("--settings") + 1];
    expect(JSON.parse(settings)).toMatchObject({ disableAllHooks: true });
  });

  it("skips permissions and MCP for isolated text sessions", () => {
    const args = buildClaudeSpawnArgs({
      isolated: true,
      maxTurns: 1,
      model: "claude-haiku-4-5",
    });
    expect(args).toContain("--no-session-persistence");
    expect(args).toContain("--strict-mcp-config");
    expect(args).toEqual(expect.arrayContaining(["--max-turns", "1"]));
    const settings = args[args.indexOf("--settings") + 1];
    expect(JSON.parse(settings)).toMatchObject({ disableAllHooks: true });
    expect(args).not.toContain("--permission-prompt-tool");
  });

  it("adds bypass flag for full-access", () => {
    const args = buildClaudeSpawnArgs({
      permissionMode: "bypassPermissions",
    });
    expect(args).toContain("--allow-dangerously-skip-permissions");
  });
});

describe("buildClaudeUserMessage", () => {
  it("embeds vision images as base64 source blocks", () => {
    const message = buildClaudeUserMessage({
      text: "look",
      attachments: [
        {
          id: "a1",
          name: "diagram.png",
          mimeType: "image/png",
          kind: "image",
          size: 4,
          data: "AQIDBA==",
        },
      ],
    });
    const content = (message.message as { content: unknown[] }).content;
    expect(content[0]).toEqual({ type: "text", text: "look" });
    expect(content[1]).toEqual({
      type: "image",
      source: {
        type: "base64",
        media_type: "image/png",
        data: "AQIDBA==",
      },
    });
  });
});

describe("control protocol", () => {
  it("parses can_use_tool requests", () => {
    const parsed = parseControlRequest({
      type: "control_request",
      request_id: "req_1",
      request: {
        subtype: "can_use_tool",
        tool_name: "Bash",
        input: { command: "ls" },
      },
    });
    expect(parsed).toMatchObject({
      requestId: "req_1",
      subtype: "can_use_tool",
      toolName: "Bash",
      input: { command: "ls" },
    });
  });

  it("maps allow/deny onto SDK permission results", () => {
    expect(toClaudePermissionResult("allow", { command: "ls" })).toEqual({
      behavior: "allow",
      updatedInput: { command: "ls" },
    });
    expect(toClaudePermissionResult("deny", {})).toMatchObject({
      behavior: "deny",
    });
  });
});

describe("stream mapping", () => {
  it("reads text and thinking deltas", () => {
    expect(
      streamDeltaFromEvent({
        type: "stream_event",
        event: {
          type: "content_block_delta",
          index: 2,
          delta: { type: "text_delta", text: "Hi" },
        },
      }),
    ).toEqual({ kind: "assistant", contentIndex: 2, text: "Hi" });
    expect(
      streamDeltaFromEvent({
        type: "stream_event",
        event: {
          type: "content_block_delta",
          index: 0,
          delta: { type: "thinking_delta", thinking: "hmm" },
        },
      }),
    ).toEqual({ kind: "reasoning", contentIndex: 0, text: "hmm" });
    expect(
      streamDeltaFromEvent({
        type: "stream_event",
        event: {
          type: "content_block_delta",
          index: 3,
          delta: { type: "text_delta", text: "\n" },
        },
      }),
    ).toEqual({ kind: "assistant", contentIndex: 3, text: "\n" });
  });

  it("keeps completed content in native index order", () => {
    expect(
      completedAssistantParts({
        message: {
          content: [
            { type: "text", text: "before" },
            { type: "tool_use", id: "toolu_1", name: "Read", input: {} },
            { type: "text", text: "after" },
          ],
        },
      }),
    ).toEqual([
      { kind: "text", contentIndex: 0, text: "before" },
      {
        kind: "tool",
        contentIndex: 1,
        use: { id: "toolu_1", name: "Read", input: {} },
      },
      { kind: "text", contentIndex: 2, text: "after" },
    ]);
  });

  it("reads tool_use content blocks", () => {
    expect(
      toolStartFromEvent({
        type: "stream_event",
        event: {
          type: "content_block_start",
          index: 1,
          content_block: {
            type: "tool_use",
            id: "toolu_1",
            name: "Read",
            input: { file_path: "a.ts" },
          },
        },
      }),
    ).toEqual({
      index: 1,
      id: "toolu_1",
      name: "Read",
      input: { file_path: "a.ts" },
    });
  });
});

describe("Claude text state", () => {
  it("reconciles streamed text by content index", () => {
    const delta = applyClaudeTextDelta(createClaudeTextState(1), {
      kind: "assistant",
      contentIndex: 0,
      text: "before",
    });
    expect(delta.event).toEqual({ type: "message.delta", text: "before" });
    expect(
      applyClaudeCompletedText(delta.state, {
        kind: "text",
        contentIndex: 0,
        text: "before",
      }).event,
    ).toBeUndefined();
  });

  it("emits missing content only after the published index", () => {
    let state = createClaudeTextState(1);
    state = markClaudeContentPublished(state, 1);
    const earlier = applyClaudeCompletedText(state, {
      kind: "text",
      contentIndex: 0,
      text: "before",
    });
    expect(earlier.conflict).toBe("position");

    const later = applyClaudeCompletedText(state, {
      kind: "text",
      contentIndex: 2,
      text: "after",
    });
    expect(later.event).toEqual({ type: "message.delta", text: "after" });
  });

  it("replays a completed text-tool-text record in index order", () => {
    const parts = completedAssistantParts({
      message: {
        content: [
          { type: "text", text: "before" },
          { type: "tool_use", id: "toolu_1", name: "Read", input: {} },
          { type: "text", text: "after" },
        ],
      },
    });
    let state = createClaudeTextState(1);
    const events: Array<{ type: string; text?: string; callId?: string }> = [];
    for (const part of parts) {
      if (part.kind === "tool") {
        state = markClaudeContentPublished(state, part.contentIndex);
        events.push({ type: "tool.started", callId: part.use.id });
        continue;
      }
      const result = applyClaudeCompletedText(state, part);
      state = result.state;
      if (result.event) events.push(result.event);
    }
    expect(events).toEqual([
      { type: "message.delta", text: "before" },
      { type: "tool.started", callId: "toolu_1" },
      { type: "message.delta", text: "after" },
    ]);
  });

  it("keeps a partial text tail when completion repeats an earlier tool", () => {
    let state = markClaudeContentPublished(createClaudeTextState(1), 1);
    state = applyClaudeTextDelta(state, {
      kind: "assistant",
      contentIndex: 2,
      text: "aft",
    }).state;

    const tool = reconcileClaudeCompletedTool(state, {
      contentIndex: 1,
      alreadyPublished: true,
    });
    const completed = applyClaudeCompletedText(tool.state, {
      kind: "text",
      contentIndex: 2,
      text: "after",
    });

    expect(tool.publish).toBe(false);
    expect(completed.event).toEqual({ type: "message.delta", text: "er" });
  });

  it("rejects a suffix after a tool invalidates the text tail", () => {
    const delta = applyClaudeTextDelta(createClaudeTextState(1), {
      kind: "assistant",
      contentIndex: 0,
      text: "hel",
    });
    const state = markClaudeContentPublished(delta.state, 1);
    const completed = applyClaudeCompletedText(state, {
      kind: "text",
      contentIndex: 0,
      text: "hello",
    });
    expect(completed.event).toBeUndefined();
    expect(completed.conflict).toBe("position");
  });
});

describe("turnStatusFromResult", () => {
  it("treats aborted terminals as interrupted", () => {
    expect(
      turnStatusFromResult({
        type: "result",
        subtype: "error_during_execution",
        terminal_reason: "aborted_streaming",
        errors: ["interrupt"],
      }).status,
    ).toBe("interrupted");
    expect(
      turnStatusFromResult({ type: "result", subtype: "success" }).status,
    ).toBe("completed");
  });
});

describe("modelsForClaudeVersion", () => {
  it("hides Opus 5 until 2.1.219", () => {
    const old = modelsForClaudeVersion("2.1.100").map(
      (model) => model.nativeId,
    );
    expect(old).not.toContain("claude-opus-5");
    expect(old).not.toContain("claude-opus-4-8");
    expect(old).toContain("claude-sonnet-4-6");

    const next = modelsForClaudeVersion("2.1.233").map(
      (model) => model.nativeId,
    );
    expect(next).toContain("claude-opus-5");
    expect(next).toContain("claude-fable-5");
    expect(next).toContain("claude-sonnet-5");
  });
});

describe("helpers", () => {
  it("parses CLI version strings", () => {
    expect(parseClaudeVersion("2.1.233 (Claude Code)")).toBe("2.1.233");
  });

  it("classifies tools and todo plans", () => {
    expect(toolKindFromName("Bash")).toBe("execute");
    expect(isTodoTool("TodoWrite")).toBe(true);
    expect(
      planTextFromTodos({
        todos: [
          { content: "One", status: "completed" },
          { content: "Two", status: "pending" },
        ],
      }),
    ).toBe("[x] One\n[ ] Two");
    expect(extractExitPlanModePlan({ plan: "# Plan" })).toBe("# Plan");
  });

  it("answers AskUserQuestion with the first option", () => {
    expect(
      askUserQuestionAllowInput({
        questions: [
          {
            question: "Which file?",
            options: [{ label: "a.ts" }, { label: "b.ts" }],
          },
        ],
      }),
    ).toMatchObject({
      answers: { "Which file?": "a.ts" },
    });
  });

  it("drops request lifecycle status pings", () => {
    expect(
      statusTextFromSystem({
        type: "system",
        subtype: "status",
        status: "requesting",
      }),
    ).toBeUndefined();
    expect(
      statusTextFromSystem({
        type: "system",
        subtype: "status",
        message: "Responding",
      }),
    ).toBeUndefined();
    expect(
      statusTextFromSystem({ type: "system", subtype: "status" }),
    ).toBeUndefined();
  });

  it("keeps status messages that carry real prose", () => {
    expect(
      statusTextFromSystem({
        type: "system",
        subtype: "status",
        message: "Retrying in 3s (rate limited)",
      }),
    ).toBe("Retrying in 3s (rate limited)");
    expect(
      statusTextFromSystem({
        type: "system",
        subtype: "compact",
        message: "Compacted context to 40k tokens",
      }),
    ).toBe("Compacted context to 40k tokens");
  });

  it("still marks a compact boundary that carries no prose", () => {
    expect(
      statusTextFromSystem({
        type: "system",
        subtype: "compact_boundary",
        compact_metadata: { trigger: "auto" },
      }),
    ).toBe("Compacted context");
  });

  it("ignores system messages that are not status or compact", () => {
    expect(
      statusTextFromSystem({
        type: "system",
        subtype: "init",
        message: "ready",
      }),
    ).toBeUndefined();
    expect(
      statusTextFromSystem({ type: "assistant", message: "hello" }),
    ).toBeUndefined();
  });
});

describe("contextUsedFromAssistant", () => {
  it("counts cached reads and writes as window occupancy", () => {
    // Shape captured from `claude --output-format stream-json --verbose`.
    const rec = {
      type: "assistant",
      message: {
        usage: {
          input_tokens: 2,
          cache_creation_input_tokens: 12941,
          cache_read_input_tokens: 16652,
          output_tokens: 3,
        },
      },
    };
    expect(contextUsedFromAssistant(rec)).toBe(29598);
  });

  it("ignores a message with no usage", () => {
    expect(
      contextUsedFromAssistant({ type: "assistant", message: {} }),
    ).toBeUndefined();
  });
});

describe("contextFromResult", () => {
  it("reads the window the CLI reports rather than a model table", () => {
    const rec = {
      type: "result",
      usage: {
        input_tokens: 2,
        cache_creation_input_tokens: 12941,
        cache_read_input_tokens: 16652,
        output_tokens: 13,
      },
      modelUsage: {
        "claude-sonnet-5": { contextWindow: 1000000, maxOutputTokens: 64000 },
      },
    };
    expect(contextFromResult(rec)).toEqual({ used: 29608, window: 1000000 });
  });

  it("uses the last iteration, since top-level usage sums the whole turn", () => {
    const rec = {
      type: "result",
      usage: {
        input_tokens: 10,
        cache_read_input_tokens: 90_000,
        output_tokens: 500,
        iterations: [
          {
            input_tokens: 5,
            cache_read_input_tokens: 20_000,
            output_tokens: 200,
          },
          {
            input_tokens: 5,
            cache_read_input_tokens: 70_000,
            output_tokens: 300,
          },
        ],
      },
      modelUsage: { "claude-opus-5": { contextWindow: 200000 } },
    };
    expect(contextFromResult(rec)).toEqual({ used: 70_305, window: 200000 });
  });

  it("has nothing to report for a turn that never called the API", () => {
    expect(contextFromResult({ type: "result", usage: {} })).toBeUndefined();
  });
});
