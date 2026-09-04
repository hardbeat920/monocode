import { describe, expect, it } from "vitest";
import { modelsForClaudeVersion, modelsFromClaudeListModels } from "./claudeCatalog";
import {
  applyClaudePromptEffortPrefix,
  askUserQuestionAllowInput,
  buildClaudeSpawnArgs,
  buildClaudeUserMessage,
  contextFromResult,
  contextUsedFromAssistant,
  extractExitPlanModePlan,
  isClaudeInitMessage,
  isSubagentMessage,
  isTodoTool,
  listModelsFromControlResponse,
  normalizeClaudeCliEffort,
  parseBackgroundAgentTasks,
  parseClaudeVersion,
  parseControlRequest,
  parseControlResponse,
  parseTaskNotification,
  parseTaskProgress,
  parseTaskStarted,
  parseTaskUpdated,
  parseToolProgress,
  taskListFromTodos,
  resolveClaudeApiModelId,
  runtimeModeToPermission,
  sessionIdFromMessage,
  statusTextFromSystem,
  streamDeltaFromEvent,
  toClaudePermissionResult,
  toolKindFromName,
  toolStartFromEvent,
  toolTitle,
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
    expect(normalizeClaudeCliEffort("ultrathink", "claude-sonnet-5")).toBeUndefined();
    expect(normalizeClaudeCliEffort("ultracode", "claude-opus-5")).toBe("xhigh");
  });

  it("maps xhigh to max on older models", () => {
    expect(normalizeClaudeCliEffort("xhigh", "claude-opus-4-6")).toBe("max");
    expect(normalizeClaudeCliEffort("xhigh", "claude-opus-5")).toBe("xhigh");
    expect(normalizeClaudeCliEffort("xhigh", "sonnet")).toBe("xhigh");
  });

  it("maps max to high on sonnet 4.6", () => {
    expect(normalizeClaudeCliEffort("max", "claude-sonnet-4-6")).toBe("high");
  });
});

describe("applyClaudePromptEffortPrefix", () => {
  it("prefixes ultrathink on the prompt", () => {
    expect(applyClaudePromptEffortPrefix("Investigate the edge cases", "ultrathink")).toBe(
      "Ultrathink:\nInvestigate the edge cases",
    );
    expect(applyClaudePromptEffortPrefix("hello", "high")).toBe("hello");
  });
});

describe("resolveClaudeApiModelId", () => {
  it("appends [1m] for the 1M context window", () => {
    expect(resolveClaudeApiModelId("claude-opus-5", "1m")).toBe("claude-opus-5[1m]");
    expect(resolveClaudeApiModelId("claude-sonnet-5", "200k")).toBe("claude-sonnet-5");
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
      expect.arrayContaining(["--model", "claude-sonnet-5", "--effort", "high"]),
    );
    expect(args).toEqual(
      expect.arrayContaining(["--permission-mode", "acceptEdits"]),
    );
    expect(args).toEqual(expect.arrayContaining(["--session-id", "sess-1"]));
    const settings = args[args.indexOf("--settings") + 1];
    expect(JSON.parse(settings).disableAllHooks).toBeUndefined();
  });

  it("only disables hooks for interactive sessions when asked", () => {
    const args = buildClaudeSpawnArgs({ settings: { disableAllHooks: true } });
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
          delta: { type: "text_delta", text: "Hi" },
        },
      }),
    ).toEqual({ kind: "assistant", text: "Hi" });
    expect(
      streamDeltaFromEvent({
        type: "stream_event",
        event: {
          type: "content_block_delta",
          delta: { type: "thinking_delta", thinking: "hmm" },
        },
      }),
    ).toEqual({ kind: "reasoning", text: "hmm" });
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
    const old = modelsForClaudeVersion("2.1.100").map((model) => model.nativeId);
    expect(old).not.toContain("claude-opus-5");
    expect(old).not.toContain("claude-opus-4-8");
    expect(old).toContain("claude-sonnet-4-6");

    const next = modelsForClaudeVersion("2.1.233").map((model) => model.nativeId);
    expect(next).toContain("claude-opus-5");
    expect(next).toContain("claude-fable-5");
    expect(next).toContain("claude-sonnet-5");
  });
});

describe("list_models catalog", () => {
  const listed = {
    type: "control_response",
    response: {
      subtype: "success",
      request_id: "list_1",
      response: {
        models: [
          {
            value: "default",
            resolvedModel: "claude-sonnet-5",
            displayName: "Default (recommended)",
            description: "Sonnet 5 · Efficient for routine tasks",
            supportsEffort: true,
            supportedEffortLevels: ["low", "medium", "high", "xhigh", "max"],
          },
          {
            value: "sonnet",
            resolvedModel: "claude-sonnet-5",
            displayName: "Sonnet",
            description: "Sonnet 5 · Efficient for routine tasks",
            supportsEffort: true,
            supportedEffortLevels: ["low", "medium", "high", "xhigh", "max"],
            supportsAdaptiveThinking: true,
          },
          {
            value: "claude-fable-5[1m]",
            resolvedModel: "claude-fable-5",
            displayName: "Fable",
            description: "Fable 5 · Most capable for your hardest tasks",
            supportsEffort: true,
            supportedEffortLevels: ["low", "medium", "high", "xhigh", "max"],
          },
          {
            value: "opus",
            resolvedModel: "claude-opus-5",
            displayName: "Opus",
            description: "Opus 5 · Best for everyday, complex tasks",
            supportsEffort: true,
            supportedEffortLevels: ["low", "medium", "high", "xhigh", "max"],
            supportsFastMode: true,
          },
          {
            value: "haiku",
            resolvedModel: "claude-haiku-4-5-20251001",
            displayName: "Haiku",
            description: "Haiku 4.5 · Fastest for quick answers",
          },
          {
            value: "cc-update-required-1",
            resolvedModel: "cc-update-required-1",
            displayName: "Fable 5.1 (disabled)",
            description: "Update to 2.1.255+ to use Fable 5.1",
            disabled: true,
          },
        ],
      },
    },
  };

  it("reads rows from the matching control response", () => {
    expect(listModelsFromControlResponse(listed, "list_1")).toHaveLength(6);
    expect(listModelsFromControlResponse(listed, "other")).toBeNull();
    expect(
      listModelsFromControlResponse(
        {
          type: "control_response",
          response: {
            subtype: "success",
            request_id: "init_1",
            response: { commands: [], models: [] },
          },
        },
        "list_1",
      ),
    ).toBeNull();
  });

  it("maps the picker catalog and drops default/disabled rows", () => {
    const models = modelsFromClaudeListModels(
      listModelsFromControlResponse(listed, "list_1"),
    );
    expect(models.map((model) => model.nativeId)).toEqual([
      "sonnet",
      "claude-fable-5",
      "opus",
      "haiku",
    ]);
    expect(models.map((model) => model.id)).toEqual([
      "claude:sonnet",
      "claude:fable-5",
      "claude:opus",
      "claude:haiku",
    ]);
    expect(models.map((model) => model.name)).toEqual([
      "Sonnet 5",
      "Fable 5",
      "Opus 5",
      "Haiku 4.5",
    ]);

    const sonnet = models[0];
    expect(sonnet?.settings?.find((setting) => setting.id === "effort")?.options.map((option) => option.value)).toEqual([
      "low",
      "medium",
      "high",
      "xhigh",
      "max",
      "ultracode",
      "ultrathink",
    ]);
    expect(sonnet?.settings?.some((setting) => setting.id === "fast")).toBe(false);

    const fable = models[1];
    expect(fable?.settings?.find((setting) => setting.id === "context")).toMatchObject({
      value: "1m",
    });

    const opus = models[2];
    expect(opus?.settings?.some((setting) => setting.id === "fast")).toBe(true);

    const haiku = models[3];
    expect(haiku?.settings).toBeUndefined();
  });

  it("parses success and error control responses", () => {
    expect(
      parseControlResponse({
        type: "control_response",
        response: {
          subtype: "success",
          request_id: "init_1",
          response: { pid: 12 },
        },
      }),
    ).toEqual({
      requestId: "init_1",
      ok: true,
      payload: { pid: 12 },
    });
    expect(
      parseControlResponse({
        type: "control_response",
        response: { subtype: "error", request_id: "list_1", error: "nope" },
      }),
    ).toEqual({
      requestId: "list_1",
      ok: false,
      payload: null,
      error: "nope",
    });
    expect(isClaudeInitMessage({ type: "system", subtype: "init" })).toBe(true);
    expect(isClaudeInitMessage({ type: "assistant", subtype: "init" })).toBe(false);
  });
});

describe("helpers", () => {
  it("parses CLI version strings", () => {
    expect(parseClaudeVersion("2.1.233 (Claude Code)")).toBe("2.1.233");
  });

  it("classifies tools and todo plans", () => {
    expect(toolKindFromName("Bash")).toBe("execute");
    expect(toolKindFromName("Skill")).toBe("skill");
    expect(toolKindFromName("Agent")).toBe("agent");
    expect(toolKindFromName("Task")).toBe("agent");
    expect(toolTitle("Agent", { description: "Explore the auth module" })).toBe(
      "Explore the auth module",
    );
    expect(toolTitle("Task", { subagent_type: "explore" })).toBe(
      "Explore subagent",
    );
    expect(toolTitle("Bash", { command: "ls -la src" })).toBe("List src");
    expect(toolTitle("Skill", { skill: "code-review" })).toBe(
      "Skill /code-review",
    );
    expect(isTodoTool("TodoWrite")).toBe(true);
    expect(toolKindFromName("TodoWrite")).toBe("tasks");
    expect(
      taskListFromTodos({
        todos: [
          { content: "One", status: "completed" },
          { content: "Two", status: "pending" },
        ],
      }),
    ).toEqual([
      { text: "One", status: "completed" },
      { text: "Two", status: "pending" },
    ]);
    expect(extractExitPlanModePlan({ plan: "# Plan" })).toBe("# Plan");
  });

  it("answers AskUserQuestion with the selected options", () => {
    const input = {
      questions: [
        {
          question: "Which file?",
          options: [{ label: "a.ts" }, { label: "b.ts" }],
        },
      ],
    };
    expect(
      askUserQuestionAllowInput(input, {
        kind: "answered",
        answers: { "Which file?": ["b.ts"] },
      }),
    ).toMatchObject({
      answers: { "Which file?": "b.ts" },
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
    expect(contextUsedFromAssistant({ type: "assistant", message: {} })).toBeUndefined();
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
          { input_tokens: 5, cache_read_input_tokens: 20_000, output_tokens: 200 },
          { input_tokens: 5, cache_read_input_tokens: 70_000, output_tokens: 300 },
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

describe("subagent messages", () => {
  it("detects nested agent traffic by parent_tool_use_id", () => {
    expect(isSubagentMessage({ parent_tool_use_id: "toolu_agent" })).toBe(true);
    expect(isSubagentMessage({ parent_tool_use_id: null })).toBe(false);
    expect(isSubagentMessage({ type: "assistant" })).toBe(false);
  });

  it("does not rebind the parent session to a subagent session id", () => {
    expect(
      sessionIdFromMessage({
        type: "assistant",
        session_id: "sub_1",
        parent_tool_use_id: "toolu_agent",
      }),
    ).toBeUndefined();
    expect(
      sessionIdFromMessage({
        type: "assistant",
        session_id: "sess_1",
        parent_tool_use_id: null,
      }),
    ).toBe("sess_1");
  });

  it("parses task lifecycle frames for local agents", () => {
    expect(
      parseTaskStarted({
        type: "system",
        subtype: "task_started",
        task_id: "t1",
        tool_use_id: "toolu_agent",
        description: "Explore the auth module",
        task_type: "local_agent",
        is_backgrounded: true,
      }),
    ).toEqual({
      taskId: "t1",
      toolUseId: "toolu_agent",
      description: "Explore the auth module",
      taskType: "local_agent",
      backgrounded: true,
      ambient: false,
    });
    expect(
      parseTaskProgress({
        type: "system",
        subtype: "task_progress",
        task_id: "t1",
        last_tool_name: "Read",
        description: "Explore the auth module",
      }),
    ).toMatchObject({
      taskId: "t1",
      lastToolName: "Read",
    });
    expect(
      parseTaskUpdated({
        type: "system",
        subtype: "task_updated",
        task_id: "t1",
        patch: { status: "completed" },
      }),
    ).toMatchObject({ taskId: "t1", status: "completed" });
    expect(
      parseTaskNotification({
        type: "system",
        subtype: "task_notification",
        task_id: "t1",
        tool_use_id: "toolu_agent",
        status: "completed",
        summary: "Found the tokens",
      }),
    ).toMatchObject({
      taskId: "t1",
      status: "completed",
      summary: "Found the tokens",
    });
    expect(
      parseBackgroundAgentTasks({
        type: "system",
        subtype: "background_tasks_changed",
        tasks: [
          {
            task_id: "t1",
            task_type: "local_agent",
            description: "Explore",
          },
          {
            task_id: "bash_1",
            task_type: "local_bash",
            description: "sleep 10",
          },
          {
            task_id: "watch",
            task_type: "local_agent",
            description: "watcher",
            ambient: true,
          },
        ],
      }),
    ).toEqual([
      { taskId: "t1", taskType: "local_agent", description: "Explore" },
    ]);
    expect(
      parseToolProgress({
        type: "tool_progress",
        tool_use_id: "toolu_agent",
        subagent_type: "explore",
      }),
    ).toMatchObject({
      toolUseId: "toolu_agent",
      subagentType: "explore",
    });
  });
});
