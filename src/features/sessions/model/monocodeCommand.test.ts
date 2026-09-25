import { describe, expect, it } from "vitest";
import {
  consumeMonocodeCommand,
  isMonocodeUserTurn,
  monocodeEnabledInThread,
  monocodeUserPrompt,
  MONOCODE_COMMAND,
} from "./monocodeCommand";

describe("MonoCode composer command", () => {
  it("exposes a local slash command", () => {
    expect(MONOCODE_COMMAND.invocation).toBe("mono");
    expect(MONOCODE_COMMAND.kind).toBe("builtin");
  });

  it("consumes only a leading standalone command", () => {
    expect(consumeMonocodeCommand("/mono list my notes")).toEqual({
      text: "list my notes",
      matched: true,
    });
    expect(consumeMonocodeCommand("  /MONO\nstart a session")).toEqual({
      text: "start a session",
      matched: true,
    });
    expect(consumeMonocodeCommand("/mono-extra list notes")).toEqual({
      text: "/mono-extra list notes",
      matched: false,
    });
    expect(consumeMonocodeCommand("Explain /mono")).toEqual({
      text: "Explain /mono",
      matched: false,
    });
    expect(consumeMonocodeCommand("/monocode list notes")).toEqual({
      text: "list notes",
      matched: true,
    });
  });

  it("keeps access for later turns when a submitted user turn enabled it", () => {
    expect(
      monocodeEnabledInThread([
        { id: "first", role: "user", text: "list notes", monocode: true },
        { id: "reply", role: "assistant", text: "Here are your notes." },
        { id: "followup", role: "user", text: "Start two sessions" },
      ]),
    ).toBe(true);
    expect(
      monocodeEnabledInThread([
        { id: "draft", role: "user", text: "list notes", draft: true, monocode: true },
        { id: "other", role: "user", text: "Explain /mono" },
      ]),
    ).toBe(false);
    const legacy = { id: "legacy", role: "user" as const, text: "/monocode list notes" };
    expect(isMonocodeUserTurn(legacy)).toBe(true);
    expect(monocodeUserPrompt(legacy)).toBe("list notes");
    expect(monocodeEnabledInThread([legacy])).toBe(true);
  });
});
