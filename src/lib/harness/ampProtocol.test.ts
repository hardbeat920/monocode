import { describe, expect, it } from "vitest";
import {
  ampModeFromModel,
  buildAmpSpawnArgs,
  buildAmpSteerMessage,
  buildAmpUserMessage,
  errorFromResult,
  isEndTurn,
  isErrorResult,
  parseJsonLine,
  sessionIdFromInit,
} from "./ampProtocol";

describe("amp spawn args", () => {
  it("includes stream-json flags and mode for a fresh session", () => {
    const args = buildAmpSpawnArgs({ mode: "medium", fast: false });
    expect(args).toContain("--execute");
    expect(args).toContain("--stream-json-thinking");
    expect(args).toContain("--stream-json-input");
    expect(args).toContain("--dangerously-allow-all");
    expect(args).toContain("--mode");
    expect(args).toContain("medium");
    expect(args).not.toContain("--fast");
    expect(args).not.toContain("threads");
  });

  it("resumes with threads continue when a thread id is given", () => {
    const args = buildAmpSpawnArgs({
      mode: "high",
      fast: true,
      threadId: "T-abc123",
    });
    expect(args.slice(0, 3)).toEqual(["threads", "continue", "T-abc123"]);
    expect(args).toContain("--fast");
    expect(args).toContain("--mode");
    expect(args).toContain("high");
  });

  it("omits mode and fast when not provided", () => {
    const args = buildAmpSpawnArgs({});
    expect(args).not.toContain("--mode");
    expect(args).not.toContain("--fast");
  });
});

describe("amp user message", () => {
  it("builds a text content block", () => {
    const msg = buildAmpUserMessage("hello world");
    expect(msg.type).toBe("user");
    const content = (msg.message as { content: unknown[] }).content;
    expect(content).toHaveLength(1);
    expect(content[0]).toEqual({ type: "text", text: "hello world" });
  });

  it("produces empty content for empty text", () => {
    const msg = buildAmpUserMessage("  ");
    const content = (msg.message as { content: unknown[] }).content;
    expect(content).toHaveLength(0);
  });
});

describe("amp steer message", () => {
  it("includes steer: true at the top level", () => {
    const msg = buildAmpSteerMessage("change direction");
    expect(msg.steer).toBe(true);
    expect(msg.type).toBe("user");
    const content = (msg.message as { content: unknown[] }).content;
    expect(content[0]).toEqual({ type: "text", text: "change direction" });
  });
});

describe("amp mode from model", () => {
  it("extracts the mode from an amp model id", () => {
    expect(ampModeFromModel("amp:low")).toBe("low");
    expect(ampModeFromModel("amp:medium")).toBe("medium");
    expect(ampModeFromModel("amp:high")).toBe("high");
    expect(ampModeFromModel("amp:ultra")).toBe("ultra");
  });

  it("returns undefined for non-amp or unknown modes", () => {
    expect(ampModeFromModel("claude:sonnet-5")).toBeUndefined();
    expect(ampModeFromModel("")).toBeUndefined();
    expect(ampModeFromModel("amp:unknown")).toBeUndefined();
  });
});

describe("amp stream parsing", () => {
  it("extracts session id from system init", () => {
    const rec = parseJsonLine(
      JSON.stringify({
        type: "system",
        subtype: "init",
        session_id: "T-abc",
        tools: [],
      }),
    );
    expect(sessionIdFromInit(rec!)).toBe("T-abc");
  });

  it("returns undefined for non-init system messages", () => {
    const rec = parseJsonLine(
      JSON.stringify({ type: "system", subtype: "status", message: "working" }),
    );
    expect(sessionIdFromInit(rec!)).toBeUndefined();
  });

  it("detects end_turn stop reason", () => {
    const rec = parseJsonLine(
      JSON.stringify({
        type: "assistant",
        message: { content: [{ type: "text", text: "done" }], stop_reason: "end_turn" },
      }),
    );
    expect(isEndTurn(rec!)).toBe(true);
  });

  it("does not treat tool_use as end of turn", () => {
    const rec = parseJsonLine(
      JSON.stringify({
        type: "assistant",
        message: { content: [], stop_reason: "tool_use" },
      }),
    );
    expect(isEndTurn(rec!)).toBe(false);
  });

  it("detects error results", () => {
    const rec = parseJsonLine(
      JSON.stringify({ type: "result", is_error: true, error: "something broke" }),
    );
    expect(isErrorResult(rec!)).toBe(true);
    expect(errorFromResult(rec!)).toBe("something broke");
  });

  it("does not treat non-error results as errors", () => {
    const rec = parseJsonLine(
      JSON.stringify({ type: "result", is_error: false }),
    );
    expect(isErrorResult(rec!)).toBe(false);
  });

  it("returns a default error message when none is present", () => {
    const rec = parseJsonLine(
      JSON.stringify({ type: "result", is_error: true }),
    );
    expect(errorFromResult(rec!)).toBe("Amp reported an error");
  });
});
