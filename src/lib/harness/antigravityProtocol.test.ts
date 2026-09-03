import { describe, expect, it } from "vitest";
import {
  buildAntigravitySpawnArgs,
  buildAntigravityUserMessage,
  mapAntigravityEvents,
  parseAntigravityLine,
  parseAntigravityModels,
} from "./antigravityProtocol";
import { DEFAULT_MODEL_ID } from "../models";
import { harnessSupportsAttachments } from "../session";

describe("antigravityProtocol", () => {
  it("does not support attachments in stream-json mode", () => {
    expect(harnessSupportsAttachments("antigravity")).toBe(false);
  });

  it("defaults to gemini-3.8-flash", () => {
    expect(DEFAULT_MODEL_ID.antigravity).toBe("antigravity:gemini-3.8-flash");
  });

  describe("buildAntigravitySpawnArgs", () => {
    it("builds stream-json flags with model and conversation", () => {
      const args = buildAntigravitySpawnArgs({
        model: "gemini-3.8-flash-high",
        conversationId: "conv-123",
        runtimeMode: "supervised",
        effort: "high",
      });
      expect(args).toEqual([
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--model",
        "gemini-3.8-flash-high",
        "--conversation",
        "conv-123",
        "--effort",
        "high",
        "--mode",
        "accept-edits",
      ]);
    });

    it("adds --dangerously-skip-permissions for full-access or auto", () => {
      const args = buildAntigravitySpawnArgs({
        model: "gemini-3.8-flash-medium",
        runtimeMode: "full-access",
      });
      expect(args).toContain("--dangerously-skip-permissions");
    });

    it("adds --mode accept-edits for auto-accept-edits", () => {
      const args = buildAntigravitySpawnArgs({
        model: "gemini-3.8-flash-low",
        runtimeMode: "auto-accept-edits",
      });
      expect(args).toContain("--mode");
      expect(args).toContain("accept-edits");
    });
  });

  describe("buildAntigravityUserMessage", () => {
    it("formats user event with message content", () => {
      const raw = buildAntigravityUserMessage({ text: "Hello Antigravity" });
      const parsed = JSON.parse(raw);
      expect(parsed).toEqual({
        event: "user",
        message: {
          role: "user",
          content: "Hello Antigravity",
        },
      });
    });
  });

  describe("parseAntigravityLine", () => {
    it("parses init event", () => {
      const line = JSON.stringify({
        event: "init",
        conversation_id: "conv-456",
        init: { model: "gemini-3.8-flash-high", cwd: "/tmp" },
      });
      expect(parseAntigravityLine(line)).toEqual({
        type: "init",
        conversationId: "conv-456",
        model: "gemini-3.8-flash-high",
      });
    });

    it("parses step_update agent_response", () => {
      const line = JSON.stringify({
        event: "step_update",
        step_update: {
          conversation_id: "conv-456",
          step_index: 1,
          state: "ACTIVE",
          step_type: "agent_response",
          text_delta: "Thinking and doing...",
          usage: { input_tokens: 100, output_tokens: 50, total_tokens: 150 },
        },
      });
      expect(parseAntigravityLine(line)).toEqual({
        type: "step_update",
        conversationId: "conv-456",
        stepIndex: 1,
        state: "ACTIVE",
        stepType: "agent_response",
        textDelta: "Thinking and doing...",
        toolName: undefined,
        toolInfo: undefined,
        durationSeconds: undefined,
        usage: {
          inputTokens: 100,
          outputTokens: 50,
          thinkingTokens: undefined,
          cacheReadTokens: undefined,
          totalTokens: 150,
        },
      });
    });

    it("parses step_update tool event with lowercase state", () => {
      const line = JSON.stringify({
        event: "step_update",
        step_update: {
          conversation_id: "conv-456",
          step_index: 2,
          state: "done",
          step_type: "tool",
          tool_name: "run_command",
          tool_info: {
            name: "run_command",
            parameters: { CommandLine: "ls -la" },
            output: "total 0",
          },
          duration_seconds: 0.12,
        },
      });
      const parsed = parseAntigravityLine(line);
      expect(parsed?.type).toBe("step_update");
      if (parsed && parsed.type === "step_update") {
        expect(parsed.state).toBe("DONE");
        expect(parsed.stepType).toBe("tool");
        expect(parsed.toolName).toBe("run_command");
        expect(parsed.durationSeconds).toBe(0.12);
        expect(parsed.toolInfo?.parameters).toEqual({ CommandLine: "ls -la" });
      }
    });

    it("parses result event with token sum fallback", () => {
      const line = JSON.stringify({
        event: "result",
        result: {
          conversation_id: "conv-456",
          status: "success",
          response: "All done!",
          duration_seconds: 1.5,
          num_turns: 1,
          usage: { input_tokens: 120, output_tokens: 80 },
        },
      });
      expect(parseAntigravityLine(line)).toEqual({
        type: "result",
        conversationId: "conv-456",
        status: "SUCCESS",
        response: "All done!",
        error: undefined,
        durationSeconds: 1.5,
        numTurns: 1,
        usage: {
          inputTokens: 120,
          outputTokens: 80,
          thinkingTokens: undefined,
          cacheReadTokens: undefined,
          totalTokens: 200,
        },
      });
    });

    it("parses top-level error event", () => {
      const line = JSON.stringify({
        event: "error",
        error: "Authentication failed",
      });
      expect(parseAntigravityLine(line)).toEqual({
        type: "error",
        error: "Authentication failed",
      });
    });

    it("returns null on non-json or malformed lines", () => {
      expect(parseAntigravityLine("not a json")).toBeNull();
      expect(parseAntigravityLine('{"event":"unknown_event"}')).toBeNull();
    });
  });

  describe("mapAntigravityEvents", () => {
    it("maps init to session.providerBound", () => {
      const events = mapAntigravityEvents({
        type: "init",
        conversationId: "conv-789",
      });
      expect(events).toEqual([
        {
          type: "session.providerBound",
          providerSessionId: "conv-789",
        },
      ]);
    });

    it("maps agent_response to message.delta and context", () => {
      const events = mapAntigravityEvents({
        type: "step_update",
        stepIndex: 1,
        state: "ACTIVE",
        stepType: "agent_response",
        textDelta: "Hello world",
        usage: { totalTokens: 42 },
      });
      expect(events).toEqual([
        { type: "message.delta", text: "Hello world" },
        { type: "context", used: 42 },
      ]);
    });

    it("maps tool step_update with scoped prefix", () => {
      const started = mapAntigravityEvents(
        {
          type: "step_update",
          stepIndex: 2,
          state: "ACTIVE",
          stepType: "tool",
          toolName: "list_dir",
          toolInfo: {
            name: "list_dir",
            parameters: { DirectoryPath: "/workspace" },
          },
        },
        "tool-sess-1-turn-1",
      );
      expect(started.length).toBe(1);
      expect(started[0].type).toBe("tool.started");
      if (started[0].type === "tool.started") {
        expect(started[0].callId).toBe("tool-sess-1-turn-1-2");
        expect(started[0].title).toBe("list_dir");
      }
    });

    it("maps error event to session.error and message.completed", () => {
      const events = mapAntigravityEvents({
        type: "error",
        error: "Fatal API error",
      });
      expect(events).toEqual([
        { type: "session.error", message: "Fatal API error" },
        { type: "message.completed" },
      ]);
    });
  });

  describe("parseAntigravityModels", () => {
    it("parses agy models tab-separated output, filtering headers and ANSI escapes", () => {
      const output = `
\x1B[32mFetching available models...\x1B[0m
MODEL ID\tNAME
---
gemini-3.8-flash-high\tGemini 3.8 Flash (High)
gemini-3.8-flash-medium\tGemini 3.8 Flash (Medium)
gemini-3.8-flash-low\tGemini 3.8 Flash (Low)
gemini-3.7-flash-high\tGemini 3.7 Flash (High)
claude-sonnet-4-6\tClaude Sonnet 4.6 (Thinking)
      `;
      const models = parseAntigravityModels(output);
      expect(models).toEqual([
        {
          id: "antigravity:gemini-3.8-flash",
          harness: "antigravity",
          name: "Gemini 3.8 Flash",
          nativeId: "gemini-3.8-flash",
          settings: [
            {
              id: "effort",
              label: "Reasoning",
              kind: "select",
              value: "high",
              options: [
                { value: "high", label: "High" },
                { value: "medium", label: "Medium" },
                { value: "low", label: "Low" },
              ],
            },
          ],
        },
        {
          id: "antigravity:gemini-3.7-flash",
          harness: "antigravity",
          name: "Gemini 3.7 Flash",
          nativeId: "gemini-3.7-flash",
          settings: [
            {
              id: "effort",
              label: "Reasoning",
              kind: "select",
              value: "high",
              options: [{ value: "high", label: "High" }],
            },
          ],
        },
        {
          id: "antigravity:claude-sonnet-4-6",
          harness: "antigravity",
          name: "Claude Sonnet 4.6 (Thinking)",
          nativeId: "claude-sonnet-4-6",
          settings: undefined,
        },
      ]);
    });
  });
});
