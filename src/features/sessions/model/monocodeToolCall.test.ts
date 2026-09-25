import { describe, expect, it } from "vitest";
import type { Block } from "./session";
import { monoCodeToolCall, monoCodeWorkSummary } from "./monocodeToolCall";

function shell(text: string): Block {
  return { id: text, role: "tool", text, tool: { kind: "shell" } };
}

describe("MonoCode CLI tool calls", () => {
  it("recognizes app actions with absolute, quoted, or bare executables", () => {
    expect(
      monoCodeToolCall(
        shell(
          "/repo/target/debug/MonoCode.app/Contents/MacOS/monocode app notes.list --json '{}'",
        ),
      )?.label,
    ).toBe("List notes");
    expect(
      monoCodeToolCall(
        shell(
          "'/Applications/MonoCode App/monocode' app folders.move --input -",
        ),
      )?.label,
    ).toBe("Move a session");
    expect(monoCodeToolCall(shell("monocode app --help"))?.label).toBe(
      "View CLI commands",
    );
    expect(
      monoCodeToolCall(shell("monocode app sessions.read --json '{}'"))?.label,
    ).toBe("Read a session");
    expect(
      monoCodeToolCall(shell("monocode app sessions.send --json '{}'"))?.label,
    ).toBe("Continue a session");
    expect(
      monoCodeToolCall(shell("monocode app sessions.draft --json '{}'"))?.label,
    ).toBe("Save a draft");
    expect(
      monoCodeToolCall({
        id: "generic",
        role: "tool",
        text: "Run command:",
        tool: { kind: "other", title: "monocode app sessions.start" },
      })?.label,
    ).toBe("Start a session");
    expect(
      monoCodeToolCall({
        id: "codex-action",
        role: "tool",
        text: "List",
        tool: {
          kind: "execute",
          preview: { kind: "shell", title: "monocode app notes.list" },
        },
      })?.label,
    ).toBe("List notes");
  });

  it("does not restyle unrelated commands or text mentioning the CLI", () => {
    expect(
      monoCodeToolCall(shell("echo monocode app notes.list")),
    ).toBeUndefined();
    expect(monoCodeToolCall(shell("monocode control list"))).toBeUndefined();
    expect(
      monoCodeToolCall({
        id: "prose",
        role: "assistant",
        text: "monocode app notes.list",
      }),
    ).toBeUndefined();
  });

  it("names a group only when all its tool calls use MonoCode", () => {
    const calls = [
      shell("monocode app --help"),
      shell("monocode app notes.list"),
    ];
    expect(monoCodeWorkSummary(calls, true)).toBe("Using MonoCode");
    expect(monoCodeWorkSummary(calls, false)).toBe("Used MonoCode");
    expect(
      monoCodeWorkSummary([...calls, shell("git status")], true),
    ).toBeUndefined();
  });
});
