import { describe, expect, it } from "vitest";
import {
  antigravityMode,
  antigravityToolKind,
  attachmentDirs,
  buildAntigravitySpawnArgs,
  buildAntigravityUserMessage,
  effectiveAntigravitySettings,
  mapAntigravityLine,
  parseAntigravityModels,
  resolveAntigravityModelWithEffort,
  unifyAntigravityCatalogModels,
} from "./antigravityProtocol";

describe("Antigravity stream-json protocol", () => {
  it("builds a documented long-lived stream invocation", () => {
    const args = buildAntigravitySpawnArgs({
      model: "antigravity:gemini-3.7-flash-high",
      modelSettings: { effort: "high", agent: "reviewer" },
      runtimeMode: "full-access",
      resume: "conversation-1",
      addDirs: ["/tmp/a", "/tmp/a", "/tmp/b"],
    });
    expect(args).toEqual(
      expect.arrayContaining([
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--disable-slash-commands",
        "--model",
        "gemini-3.7-flash-high",
        "--effort",
        "high",
        "--agent",
        "reviewer",
        "--conversation",
        "conversation-1",
        "--dangerously-skip-permissions",
      ]),
    );
    expect(args.filter((value) => value === "--add-dir")).toHaveLength(2);
  });

  it("includes working directory in --add-dir arguments", () => {
    const args = buildAntigravitySpawnArgs({
      model: "antigravity:default",
      runtimeMode: "supervised",
      cwd: "/repo/workspace",
      addDirs: ["/repo/workspace", "/other/dir"],
    });
    expect(args).toEqual(
      expect.arrayContaining([
        "--add-dir",
        "/repo/workspace",
        "--add-dir",
        "/other/dir",
      ]),
    );
    expect(args.filter((value) => value === "--add-dir")).toHaveLength(2);
  });

  it("maps only supported execution modes", () => {
    expect(antigravityMode("supervised")).toBeUndefined();
    expect(antigravityMode("auto")).toBeUndefined();
    expect(antigravityMode("auto-accept-edits")).toBe("accept-edits");
  });

  it("sandboxes normal sessions and leaves full access explicit", () => {
    expect(
      buildAntigravitySpawnArgs({
        model: "antigravity:default",
        runtimeMode: "supervised",
      }),
    ).toEqual(expect.arrayContaining(["--sandbox"]));
    expect(
      buildAntigravitySpawnArgs({
        model: "antigravity:default",
        runtimeMode: "full-access",
      }),
    ).toEqual(expect.arrayContaining(["--dangerously-skip-permissions"]));
    expect(
      buildAntigravitySpawnArgs({
        model: "antigravity:default",
        runtimeMode: "full-access",
      }),
    ).not.toEqual(expect.arrayContaining(["--sandbox"]));
  });

  it("uses text-only @ path references for attachments and resolves relative paths", () => {
    const attachments = [
      {
        id: "image1",
        name: "diagram.png",
        mimeType: "image/png" as const,
        kind: "image" as const,
        size: 5,
        path: "/tmp/attachments/diagram.png",
        data: "not-sent",
      },
      {
        id: "image2",
        name: "screenshot.jpg",
        mimeType: "image/jpeg" as const,
        kind: "image" as const,
        size: 10,
        path: "docs/screenshot.jpg",
      },
    ];
    expect(
      buildAntigravityUserMessage({
        text: "inspect",
        cwd: "/Users/dev/project",
        attachments,
      }),
    ).toEqual({
      event: "user",
      message: {
        content:
          "Attachments to inspect:\n@[/tmp/attachments/diagram.png]\n@[/Users/dev/project/docs/screenshot.jpg]\n\ninspect",
      },
    });
    expect(attachmentDirs(attachments)).toEqual([
      "/tmp/attachments",
      "docs",
    ]);
  });

  it("parses init, deltas, tool outcomes and a terminal result with context usage", () => {
    expect(
      mapAntigravityLine({ event: "init", conversation_id: "c1", init: {} }),
    ).toMatchObject({ initialized: "c1", providerSessionId: "c1" });
    expect(
      mapAntigravityLine({
        event: "step_update",
        step_update: {
          conversation_id: "c1",
          step_index: 1,
          state: "ACTIVE",
          step_type: "agent_response",
          thinking_delta: "pondering...",
          text_delta: "hello",
        },
      }).events,
    ).toEqual([
      { type: "reasoning.delta", text: "pondering..." },
      { type: "message.delta", text: "hello" },
    ]);
    const tool = mapAntigravityLine({
      event: "step_update",
      step_update: {
        conversation_id: "c1",
        step_index: 3,
        state: "DONE",
        step_type: "tool",
        tool_name: "run_command",
        tool_info: {
          parameters: { CommandLine: "git status" },
          output: "clean",
        },
      },
    }).events[0];
    expect(tool).toMatchObject({
      type: "tool.updated",
      callId: "c1:3",
      kind: "execute",
      status: "completed",
      title: "git status",
    });

    const editTool = mapAntigravityLine({
      event: "step_update",
      step_update: {
        conversation_id: "c1",
        step_index: 4,
        state: "DONE",
        step_type: "tool",
        tool_name: "replace_file_content",
        tool_info: {
          parameters: {
            TargetFile: "/repo/src/index.ts",
            TargetContent: "const a = 1;",
            ReplacementContent: "const a = 2;",
          },
          output: "ok",
        },
      },
    }).events[0];
    expect(editTool).toMatchObject({
      type: "tool.updated",
      callId: "c1:4",
      kind: "edit",
      status: "completed",
      title: "Edit /repo/src/index.ts",
      preview: {
        kind: "write",
        fileName: "index.ts",
        path: "/repo/src/index.ts",
        additions: 1,
        deletions: 1,
      },
    });

    const result = mapAntigravityLine(
      {
        event: "result",
        result: {
          conversation_id: "c1",
          status: "SUCCESS",
          response: "hello world",
          usage: {
            total_tokens: 1250,
            input_tokens: 1000,
            output_tokens: 250,
          },
        },
      },
      "hello",
    );
    expect(result.events).toEqual(
      expect.arrayContaining([
        { type: "message.delta", text: " world" },
        { type: "message.completed" },
        { type: "reasoning.completed" },
        { type: "context", used: 1250, window: 1_000_000 },
      ]),
    );
    expect(result.turnCompleted).toEqual({ ok: true, response: "hello world" });
  });

  it("retains errors and does not emit error text as message delta", () => {
    const result = mapAntigravityLine({
      event: "result",
      result: {
        status: "ERROR",
        error: "timeout waiting for response",
        response: "timeout waiting for response",
      },
    });
    // Crucial: do not emit message.delta on error so error is not duplicated
    expect(result.events.find((e) => e.type === "message.delta")).toBeUndefined();
    expect(result.turnCompleted).toEqual({
      ok: false,
      error: "timeout waiting for response",
      response: "timeout waiting for response",
    });
    expect(antigravityToolKind("write_to_file")).toBe("edit");
    expect(antigravityToolKind("grep_search")).toBe("search");
    expect(antigravityToolKind("invoke_subagent")).toBe("agent");
  });

  it("reads the structured model catalog even when multi-line output is returned", () => {
    const multiLineOutput = `Fetching available models...\n${JSON.stringify({
      command: {
        data: {
          models: [
            {
              id: "gemini-3.7-flash-high",
              label: "Gemini 3.7 Flash (High)",
            },
            {
              id: "claude-sonnet-4-6",
              label: "Claude Sonnet 4.6 (Thinking)",
            },
          ],
        },
      },
    })}`;
    expect(parseAntigravityModels(multiLineOutput)).toEqual([
      { id: "gemini-3.7-flash-high", label: "Gemini 3.7 Flash (High)" },
      { id: "claude-sonnet-4-6", label: "Claude Sonnet 4.6 (Thinking)" },
    ]);
  });

  it("extracts thinking and content snapshots and uses toolAction for weak tool names", () => {
    // 1. Thinking snapshot
    const thoughtStep = mapAntigravityLine(
      {
        event: "step_update",
        step_update: {
          step_type: "thought",
          thinking: "Initial investigation of the codebase...",
        },
      },
      "",
      undefined,
      "",
    );
    expect(thoughtStep.events).toEqual([
      {
        type: "reasoning.delta",
        text: "Initial investigation of the codebase...",
      },
    ]);

    // 2. Content snapshot without prior delta
    const contentStep = mapAntigravityLine(
      {
        event: "step_update",
        step_update: {
          step_type: "agent_response",
          content: "I will now check the git status.",
        },
      },
      "",
      undefined,
      "",
    );
    expect(contentStep.events).toEqual([
      { type: "message.delta", text: "I will now check the git status." },
    ]);

    // 3. Tool step with toolAction replaces weak tool title
    const toolStep = mapAntigravityLine({
      event: "step_update",
      step_update: {
        step_type: "tool",
        tool_name: "list_dir",
        step_index: 2,
        state: "ACTIVE",
        tool_info: {
          parameters: {
            DirectoryPath: "/projects/my-app",
            toolAction: "Listing project files",
            toolSummary: "List files",
          },
        },
      },
    });
    expect(toolStep.events).toEqual([
      expect.objectContaining({
        type: "tool.started",
        title: "Listing project files",
        kind: "read",
      }),
    ]);
  });

  it("resolves unified models with reasoning effort picker values", () => {
    // Gemini 3.8 Flash
    expect(resolveAntigravityModelWithEffort("antigravity:gemini-3.8-flash", "high")).toEqual({
      model: "gemini-3.8-flash-high",
      effort: "high",
    });
    expect(resolveAntigravityModelWithEffort("antigravity:gemini-3.8-flash", "medium")).toEqual({
      model: "gemini-3.8-flash-medium",
      effort: "medium",
    });
    expect(resolveAntigravityModelWithEffort("antigravity:gemini-3.8-flash", "low")).toEqual({
      model: "gemini-3.8-flash-low",
      effort: "low",
    });

    // Gemini 3.1 Pro
    expect(resolveAntigravityModelWithEffort("antigravity:gemini-3.1-pro", "high")).toEqual({
      model: "gemini-3.1-pro-high",
      effort: "high",
    });
    expect(resolveAntigravityModelWithEffort("antigravity:gemini-3.1-pro", "low")).toEqual({
      model: "gemini-3.1-pro-low",
      effort: "low",
    });

    // Default
    expect(resolveAntigravityModelWithEffort("antigravity:default")).toEqual({
      model: "",
      effort: undefined,
    });
  });

  it("activates high effort when /boost is present in prompt", () => {
    const args = buildAntigravitySpawnArgs({
      model: "antigravity:gemini-3.8-flash",
      runtimeMode: "supervised",
      prompt: "Refactor database /boost",
    });
    expect(args).toEqual(
      expect.arrayContaining(["--model", "gemini-3.8-flash-high", "--effort", "high"]),
    );
  });

  it("activates --mode plan when /plan is present in prompt", () => {
    const args = buildAntigravitySpawnArgs({
      model: "antigravity:gemini-3.8-flash",
      runtimeMode: "supervised",
      prompt: "Architect the auth system /plan",
    });
    expect(args).toEqual(
      expect.arrayContaining(["--mode", "plan"]),
    );
  });

  it("extracts effective settings with prompt overrides for multi-turn validation", () => {
    const normal = effectiveAntigravitySettings({
      model: "antigravity:gemini-3.8-flash",
      runtimeMode: "supervised",
      prompt: "normal prompt",
    });
    expect(normal).toEqual({
      model: "gemini-3.8-flash-high",
      effort: "high",
      agent: undefined,
      mode: undefined,
    });

    const boostedAndPlanned = effectiveAntigravitySettings({
      model: "antigravity:gemini-3.8-flash",
      runtimeMode: "supervised",
      prompt: "plan and boost /plan /boost",
    });
    expect(boostedAndPlanned).toEqual({
      model: "gemini-3.8-flash-high",
      effort: "high",
      agent: undefined,
      mode: "plan",
    });
  });

  it("maps ask_question tool steps to question.asked and question.resolved", () => {
    const asked = mapAntigravityLine({
      event: "step_update",
      step_update: {
        step_type: "tool",
        tool_name: "ask_question",
        step_index: 5,
        state: "ACTIVE",
        tool_info: {
          parameters: {
            questions: [
              {
                question: "Which database engine would you prefer?",
                options: ["PostgreSQL", "SQLite"],
                is_multi_select: false,
              },
            ],
          },
        },
      },
    });
    expect(asked.events).toEqual([
      expect.objectContaining({
        type: "question.asked",
        requestId: 5,
        questions: expect.arrayContaining([
          expect.objectContaining({
            prompt: "Which database engine would you prefer?",
            options: expect.arrayContaining([
              expect.objectContaining({ label: "PostgreSQL" }),
              expect.objectContaining({ label: "SQLite" }),
            ]),
          }),
        ]),
      }),
      expect.objectContaining({
        type: "tool.started",
        kind: "other",
      }),
    ]);

    const resolved = mapAntigravityLine({
      event: "step_update",
      step_update: {
        step_type: "tool",
        tool_name: "ask_question",
        step_index: 5,
        state: "DONE",
        tool_info: {
          parameters: {
            questions: [
              {
                question: "Which database engine would you prefer?",
                options: ["PostgreSQL", "SQLite"],
              },
            ],
          },
        },
      },
    });
    expect(resolved.events).toEqual([
      expect.objectContaining({
        type: "question.resolved",
        requestId: 5,
        decision: "answered",
      }),
      expect.objectContaining({
        type: "tool.updated",
        status: "completed",
      }),
    ]);
  });

  it("maps plan artifact generation to plan event", () => {
    const planStep = mapAntigravityLine({
      event: "step_update",
      step_update: {
        step_type: "tool",
        tool_name: "write_to_file",
        step_index: 2,
        state: "ACTIVE",
        tool_info: {
          parameters: {
            TargetFile: "/repo/plan.md",
            CodeContent: "# Implementation Plan\n\n1. Setup\n2. Build",
          },
        },
      },
    });
    expect(planStep.events).toEqual(
      expect.arrayContaining([
        {
          type: "plan",
          text: "# Implementation Plan\n\n1. Setup\n2. Build",
        },
      ]),
    );
  });

  it("unifies catalog models by family and groups reasoning efforts", () => {
    const raw = [
      { id: "gemini-3.8-flash-high", label: "Gemini 3.8 Flash (High)" },
      { id: "gemini-3.8-flash-medium", label: "Gemini 3.8 Flash (Medium)" },
      { id: "gemini-3.8-flash-low", label: "Gemini 3.8 Flash (Low)" },
      { id: "claude-sonnet-4-6", label: "Claude Sonnet 4.6 (Thinking)" },
    ];
    const unified = unifyAntigravityCatalogModels(raw);
    expect(unified).toHaveLength(2);
    const flash = unified.find((m) => m.id === "antigravity:gemini-3.8-flash");
    expect(flash).toBeDefined();
    expect(flash?.name).toBe("Gemini 3.8 Flash");
    expect(flash?.settings?.[0]?.options).toEqual([
      { label: "High", value: "high" },
      { label: "Medium", value: "medium" },
      { label: "Low", value: "low" },
    ]);
    const claude = unified.find((m) => m.id === "antigravity:claude-sonnet-4-6");
    expect(claude).toBeDefined();
    expect(claude?.settings).toBeUndefined();
  });
});
