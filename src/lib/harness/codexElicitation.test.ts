import { describe, expect, it } from "vitest";
import {
  codexMcpConfirmation,
  isCodexComputerUseAccessConfirmation,
} from "./codexElicitation";

const confirmation = {
  mode: "form",
  serverName: "example",
  message: "Confirm access",
  requestedSchema: {
    type: "object",
    properties: {
      approved: { type: "boolean", title: "Read this source?", default: false },
    },
    required: ["approved"],
    additionalProperties: false,
  },
};

describe("Codex MCP confirmations", () => {
  it.each(["form", "openai/form", "openaiForm"])(
    "supports a Boolean confirmation in %s mode",
    (mode) => {
      expect(codexMcpConfirmation({ ...confirmation, mode })).toEqual({
        title: "example: Confirm access — Read this source?",
        content: { approved: true },
      });
    },
  );

  it("keeps empty confirmations compatible", () => {
    expect(
      codexMcpConfirmation({
        ...confirmation,
        requestedSchema: { type: "object", properties: {}, required: [] },
      })?.content,
    ).toEqual({});
  });

  it.each([
    { type: "object", properties: {}, required: ["missing"] },
    { type: "object", properties: {}, required: "approved" },
    { type: "object", properties: { approved: { type: "string" } } },
    {
      type: "object",
      properties: { approved: { type: "boolean", const: false } },
    },
    {
      type: "object",
      properties: { approved: { type: "boolean" }, name: { type: "string" } },
    },
    { ...confirmation.requestedSchema, allOf: [{ required: ["missing"] }] },
  ])(
    "does not invent answers for unsupported schemas: %j",
    (requestedSchema) => {
      expect(
        codexMcpConfirmation({ ...confirmation, requestedSchema }),
      ).toBeNull();
    },
  );

  it("does not treat browser authorization as a confirmation", () => {
    expect(codexMcpConfirmation({ ...confirmation, mode: "url" })).toBeNull();
  });

  it("identifies computer-use app access without matching other confirmations", () => {
    expect(
      isCodexComputerUseAccessConfirmation({
        ...confirmation,
        serverName: "cua_repl",
        message: 'Allow Computer Use to use "QuickTime Player"?',
      }),
    ).toBe(true);
    expect(isCodexComputerUseAccessConfirmation(confirmation)).toBe(false);
    expect(
      isCodexComputerUseAccessConfirmation({
        ...confirmation,
        serverName: "cua_repl",
        message: "Allow this form submission?",
      }),
    ).toBe(false);
  });
});
