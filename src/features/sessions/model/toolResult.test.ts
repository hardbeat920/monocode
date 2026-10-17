import { describe, expect, it } from "vitest";
import type { Block } from "./session";
import { successfulToolOutput } from "./toolResult";

const call = (tool: Block["tool"]): Block => ({ id: "call", role: "tool", text: "Run command", tool });

describe("successful tool output", () => {
  it("prefers explicit preview output and preserves its whitespace", () => {
    expect(successfulToolOutput(call({ status: "completed", detail: "short", detailIsOutput: true, preview: { kind: "shell", output: " full output\n" } }))).toBe(" full output\n");
  });
  it("uses detail only when recorded as a returned result", () => {
    expect(successfulToolOutput(call({ status: "completed", detail: '{"command":"input"}' }))).toBeUndefined();
    expect(successfulToolOutput(call({ status: "completed", detail: "actual result", detailIsOutput: true }))).toBe("actual result");
    expect(successfulToolOutput(call({ status: "success", detail: "actual result", detailIsOutput: true }))).toBe("actual result");
  });
  it.each(["in_progress", "failed", "cancelled"])("does not expose a successful-result panel for %s", (status) => {
    expect(successfulToolOutput(call({ status, preview: { kind: "shell", output: "output" } }))).toBeUndefined();
  });
  it("omits empty output and calls awaiting approval", () => {
    const block = call({ status: "completed", detailIsOutput: true, detail: " ", preview: { kind: "shell", output: "\n" } });
    expect(successfulToolOutput(block)).toBeUndefined();
    block.tool!.detail = "output";
    block.approval = { requestId: 1 };
    expect(successfulToolOutput(block)).toBeUndefined();
  });
  it("never interprets an unresolved preview as a successful result", () => {
    expect(successfulToolOutput(call({ preview: { kind: "shell", output: "input arguments" } }))).toBeUndefined();
    expect(successfulToolOutput(call({ status: "in_progress", detailIsOutput: true, detail: "stale output" }))).toBeUndefined();
    expect(successfulToolOutput(call({ status: "completed", preview: { kind: "shell", output: "\n  " }, detail: '{"command":"input"}' }))).toBeUndefined();
  });
});
