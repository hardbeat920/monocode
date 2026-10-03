import { describe, expect, it } from "vitest";
import { elicitationQuestions, elicitationResponse } from "./claudeElicitation";

const request = {
  mode: "form",
  requested_schema: {
    type: "object",
    required: ["label", "count"],
    properties: {
      label: { type: "string", minLength: 2 },
      count: { type: "integer", minimum: 1, maximum: 10 },
      enabled: { type: "boolean" },
    },
  },
};

describe("MCP elicitation", () => {
  it("preserves a schema field that uses the action question's default ID", () => {
    const form = {
      requested_schema: { properties: { __mcp_action: { type: "string" } } },
    };
    expect(
      elicitationResponse(form, elicitationQuestions(form), {
        kind: "answered",
        answers: { __mcp_action_: ["accept"] },
        custom: { __mcp_action: "value" },
      }),
    ).toEqual({ action: "accept", content: { __mcp_action: "value" } });
  });
  it("returns typed answers and keeps optional omissions absent", () => {
    const questions = elicitationQuestions(request);
    const reply = {
      kind: "answered" as const,
      answers: { __mcp_action: ["accept"] },
      custom: { label: "Test", count: "3" },
    };
    expect(elicitationResponse(request, questions, reply)).toEqual({
      action: "accept",
      content: { label: "Test", count: 3 },
    });
  });
  it.each(["decline", "cancel"])("preserves the user's %s action", (action) => {
    expect(
      elicitationResponse(request, elicitationQuestions(request), {
        kind: "answered",
        answers: { __mcp_action: [action] },
      }),
    ).toEqual({ action });
  });
  it("cancels a skipped form", () => {
    expect(
      elicitationResponse(request, elicitationQuestions(request), {
        kind: "skipped",
      }),
    ).toEqual({ action: "cancel" });
  });
  it.each(["0", "11", "1.5", "invalid"])(
    "rejects an invalid integer %s",
    (count) => {
      expect(() =>
        elicitationResponse(request, elicitationQuestions(request), {
          kind: "answered",
          answers: { __mcp_action: ["accept"] },
          custom: { label: "Test", count },
        }),
      ).toThrow();
    },
  );
  it("rejects missing required answers", () => {
    expect(() =>
      elicitationResponse(request, elicitationQuestions(request), {
        kind: "answered",
        answers: { __mcp_action: ["accept"] },
      }),
    ).toThrow("required");
  });
});
