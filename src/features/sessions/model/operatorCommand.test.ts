import { describe, expect, it } from "vitest";
import {
  consumeOperatorCommand,
  isOperatorUserTurn,
  operatorEnabledInThread,
  operatorUserPrompt,
  OPERATOR_COMMAND,
} from "./operatorCommand";

describe("Operator composer command", () => {
  it("exposes a local slash command", () => {
    expect(OPERATOR_COMMAND.invocation).toBe("operator");
    expect(OPERATOR_COMMAND.kind).toBe("builtin");
  });

  it("consumes only a leading standalone command", () => {
    expect(consumeOperatorCommand("/operator list my notes")).toEqual({
      text: "list my notes",
      matched: true,
    });
    expect(consumeOperatorCommand("  /OPERATOR\nstart a session")).toEqual({
      text: "start a session",
      matched: true,
    });
    expect(consumeOperatorCommand("/operator-extra list notes")).toEqual({
      text: "/operator-extra list notes",
      matched: false,
    });
    expect(consumeOperatorCommand("Explain /operator")).toEqual({
      text: "Explain /operator",
      matched: false,
    });
    for (const alias of ["/mono", "/monocode"]) {
      expect(consumeOperatorCommand(`${alias} list notes`)).toEqual({
        text: "list notes",
        matched: true,
      });
    }
  });

  it("keeps access for later turns when a submitted user turn enabled it", () => {
    expect(
      operatorEnabledInThread([
        { id: "first", role: "user", text: "list notes", monocode: true },
        { id: "reply", role: "assistant", text: "Here are your notes." },
        { id: "followup", role: "user", text: "Start two sessions" },
      ]),
    ).toBe(true);
    expect(
      operatorEnabledInThread([
        {
          id: "draft",
          role: "user",
          text: "list notes",
          draft: true,
          monocode: true,
        },
        { id: "other", role: "user", text: "Explain /operator" },
      ]),
    ).toBe(false);
    for (const alias of ["/mono", "/monocode"]) {
      const legacy = {
        id: "legacy",
        role: "user" as const,
        text: `${alias} list notes`,
      };
      expect(isOperatorUserTurn(legacy)).toBe(true);
      expect(operatorUserPrompt(legacy)).toBe("list notes");
      expect(operatorEnabledInThread([legacy])).toBe(true);
    }
  });
});
