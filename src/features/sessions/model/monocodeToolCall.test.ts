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
    expect(
      monoCodeToolCall(
        shell('"C:\\Program Files\\MonoCode\\monocode.exe" app notes.list'),
      )?.label,
    ).toBe("List notes");
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

  it("does not compact compound shell commands or hide a longer shell preview", () => {
    for (const command of [
      "monocode app notes.list && echo extra",
      "monocode app notes.list; echo extra",
      "monocode app notes.list | cat",
      "monocode app notes.list\necho extra",
      "monocode app notes.list --json \"$(echo extra)\"",
    ]) {
      expect(monoCodeToolCall(shell(command))).toBeUndefined();
    }
    expect(
      monoCodeToolCall({
        id: "short-title",
        role: "tool",
        text: "monocode app notes.list",
        tool: {
          kind: "shell",
          title: "monocode app notes.list",
          preview: {
            kind: "shell",
            title: "monocode app notes.list && echo extra",
          },
        },
      }),
    ).toBeUndefined();
    expect(
      monoCodeToolCall(
        shell("monocode app sessions.send --json '{\"prompt\":\"a; b\"}'"),
      )?.command,
    ).toBe("monocode app sessions.send --json '{\"prompt\":\"a; b\"}'");
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
