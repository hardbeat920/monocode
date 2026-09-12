import { describe, expect, it } from "vitest";
import {
  composeToolTitle,
  extractSearchQuery,
  extractShellCommand,
  extractSkillName,
  extractToolPreview,
  isWeakToolTitle,
  MAX_PREVIEW_LINES,
  mergeToolPreview,
  titleFromToolInput,
} from "./preview";
import { previewFromTool } from "./claudeProtocol";

describe("tool input change previews", () => {
  it("compares Claude Edit excerpts outside the workspace without inventing file line numbers", () => {
    const preview = previewFromTool("Edit", {
      file_path: "/Users/me/Documents/notes.md",
      old_string: "  before\nkeep\n",
      new_string: "  after\nkeep\n",
    });
    expect(preview).toMatchObject({
      kind: "write",
      path: "/Users/me/Documents/notes.md",
      additions: 1,
      deletions: 1,
      lines: [
        { kind: "del", text: "  before" },
        { kind: "add", text: "  after" },
        { kind: "context", text: "keep" },
      ],
    });
    expect(preview?.lines?.every((line) => line.number === undefined)).toBe(
      true,
    );
  });

  it("preserves whitespace-only replacements, insertions and deletions", () => {
    for (const [before, after, additions, deletions] of [
      ["  ", "    ", 1, 1],
      ["", "new\n", 1, 0],
      ["old\n", "", 0, 1],
      ["same\n", "same\n", 0, 0],
    ] as const) {
      expect(
        previewFromTool("Edit", {
          file_path: "notes.md",
          old_string: before,
          new_string: after,
        }),
      ).toMatchObject({ additions, deletions });
    }
  });

  it("reads complete nested replacement inputs but waits for both strings", () => {
    expect(
      extractToolPreview(
        {
          kind: "edit",
          args: { input: { path: "a.ts", oldText: "old", newText: "new" } },
        },
        {},
      ),
    ).toMatchObject({ additions: 1, deletions: 1 });
    expect(
      previewFromTool("Edit", {
        file_path: "a.ts",
        old_string: "old",
      })?.lines,
    ).toBeUndefined();
  });

  it("finds edits after long unchanged prefixes and counts beyond the preview", () => {
    const prefix = Array.from({ length: 900 }, (_, i) => `line ${i}`).join(
      "\n",
    );
    const preview = previewFromTool("Edit", {
      file_path: "long.md",
      old_string: prefix,
      new_string: `${prefix}\n${Array(20).fill("added").join("\n")}`,
    });
    expect(preview).toMatchObject({ additions: 20, deletions: 0 });
    expect(preview?.lines).toHaveLength(MAX_PREVIEW_LINES);
    expect(preview?.lines?.some((line) => line.text === "added")).toBe(true);
  });

  it("shows Write contents without claiming to know the previous file", () => {
    const preview = previewFromTool(
      "Write",
      {
        file_path: "notes.md",
        content: "  heading\n\nbody\n",
      },
      "File successfully written",
    );
    expect(preview).toMatchObject({
      contentOnly: true,
      lines: [
        { number: 1, kind: "context", text: "  heading" },
        { number: 2, kind: "context", text: "" },
        { number: 3, kind: "context", text: "body" },
      ],
    });
    expect(preview?.additions).toBeUndefined();
    expect(preview?.deletions).toBeUndefined();
    const empty = previewFromTool("Write", {
      file_path: "notes.md",
      content: "",
    });
    expect(mergeToolPreview(empty, preview)?.lines).toEqual([]);
    expect(
      previewFromTool("Read", { file_path: "notes.md" }, "contents")?.lines,
    ).toBeUndefined();
  });

  it("prefers provider diffs and preserves indentation in them", () => {
    const preview = extractToolPreview(
      {
        kind: "edit",
        input: { old_string: "wrong", new_string: "fallback" },
        content: [
          {
            type: "diff",
            path: "a.ts",
            oldText: "  old\n",
            newText: "  new\n",
          },
        ],
      },
      {},
    );
    expect(preview?.lines?.map((line) => line.text)).toEqual([
      "  old",
      "  new",
    ]);
  });
});

describe("extractToolPreview", () => {
  it("reads nested args bags from ACP tool calls", () => {
    const preview = extractToolPreview(
      {
        kind: "read",
        title: "Read",
        rawInput: {
          args: { path: "src/chrome/TitleBar.tsx" },
        },
      },
      {},
    );
    expect(preview).toMatchObject({
      kind: "read",
      path: "src/chrome/TitleBar.tsx",
      fileName: "TitleBar.tsx",
    });
    expect(
      composeToolTitle({
        kind: "read",
        title: "Read",
        path: preview?.path,
        previewKind: preview?.kind,
      }),
    ).toBe("Read src/chrome/TitleBar.tsx");
  });

  it("accepts relative single-segment paths", () => {
    const preview = extractToolPreview(
      { kind: "read", title: "Read", rawInput: { path: "README.md" } },
      {},
    );
    expect(preview?.path).toBe("README.md");
  });

  it("does not treat file contents as a path", () => {
    const preview = extractToolPreview(
      {
        kind: "read",
        title: "Read",
        rawInput: { path: "/** Structured language…" },
      },
      {},
    );
    expect(preview?.path).toBeUndefined();
  });

  it("finds search queries in nested input", () => {
    expect(
      extractSearchQuery([
        { arguments: { pattern: "busyHarness|busy.*tab" } },
      ]),
    ).toBe("busyHarness|busy.*tab");
  });

  it("uses locations when rawInput is empty", () => {
    const preview = extractToolPreview(
      {
        kind: "read",
        title: "Read",
        locations: [{ path: "src/App.tsx" }],
      },
      {},
    );
    expect(preview?.path).toBe("src/App.tsx");
  });

  it("treats glob_pattern as a Find query", () => {
    const preview = extractToolPreview(
      {
        kind: "search",
        title: "Find",
        name: "Glob",
        rawInput: { glob_pattern: "**/*.{json,md}" },
      },
      {},
    );
    expect(preview).toMatchObject({
      kind: "search",
      query: "**/*.{json,md}",
    });
    expect(
      composeToolTitle({
        kind: "search",
        title: "Find",
        query: preview?.query,
        previewKind: preview?.kind,
      }),
    ).toBe("Find **/*.{json,md}");
  });
});

describe("shell command titles", () => {
  it("pulls the command out of nested input bags", () => {
    expect(extractShellCommand({ args: { command: "git status -s" } })).toBe(
      "git status -s",
    );
    expect(extractShellCommand({ cmd: "pwd" })).toBe("pwd");
    expect(extractShellCommand({ command: ["npm", "test"] })).toBe("npm test");
    expect(
      extractShellCommand({}, { rawInput: {} }, { input: { command: "git status" } }),
    ).toBe("git status");
  });

  it("labels execute tools with the command, not the tool name", () => {
    expect(
      composeToolTitle({
        kind: "execute",
        title: "Bash",
        command: "rm -rf /tmp/build",
      }),
    ).toBe("rm -rf /tmp/build");
    expect(titleFromToolInput("bash", "execute", { command: "ls -la" })).toBe(
      "ls -la",
    );
    expect(
      titleFromToolInput("Bash", "execute", {
        description: "List files",
        command: "ls -la",
      }),
    ).toBe("ls -la");
    expect(
      titleFromToolInput("bash", "execute", {
        command: "cat src/lib/harness/preview.ts",
      }),
    ).toBe("Read src/lib/harness/preview.ts");
    expect(
      titleFromToolInput("Bash", "execute", {
        command:
          "sed -n '713,1200p' src/surfaces/AgentTranscript.tsx",
      }),
    ).toBe("Read src/surfaces/AgentTranscript.tsx");
    expect(
      titleFromToolInput("bash", "execute", {
        command: 'grep -n "isReadTool" src/lib/harness/preview.ts',
      }),
    ).toBe("Find isReadTool");
    expect(
      titleFromToolInput("bash", "execute", {
        command: "git diff src/surfaces/AgentTranscript.tsx",
      }),
    ).toBe("git diff src/surfaces/AgentTranscript.tsx");
    expect(
      titleFromToolInput("bash", "execute", {
        command: "sed -i 's/a/b/' src/app.ts",
      }),
    ).toBe("Edit src/app.ts");
    expect(
      titleFromToolInput("bash", "execute", {
        command: "cat package.json > out.json",
      }),
    ).toBe("Write out.json");
  });

  it("does not substitute Claude's description for the command", () => {
    expect(
      titleFromToolInput("Bash", "execute", { description: "List files" }),
    ).toBe("Shell");
    expect(titleFromToolInput("bash", "execute", {})).toBe("Shell");
  });

  it("treats Bash/bash as a weak placeholder so a later command can replace it", () => {
    expect(isWeakToolTitle("Bash")).toBe(true);
    expect(isWeakToolTitle("bash")).toBe(true);
    expect(isWeakToolTitle("ls")).toBe(false);
  });
});

describe("skill titles", () => {
  it("labels a Skill tool with /name", () => {
    expect(extractSkillName({ skill: "code-review" })).toBe("code-review");
    expect(extractSkillName({ args: { skill_name: "commit" } })).toBe("commit");
    expect(extractSkillName({}, { rawInput: {} }, { skill: "code-review" })).toBe(
      "code-review",
    );
    expect(titleFromToolInput("Skill", "skill", { skill: "code-review" })).toBe(
      "Skill /code-review",
    );
    expect(
      composeToolTitle({
        kind: "skill",
        title: "Skill",
        skill: "code-review",
      }),
    ).toBe("Skill /code-review");
  });

  it("does not treat a bare Skill placeholder as the name", () => {
    expect(titleFromToolInput("Skill", "skill", {})).toBe("Skill");
    expect(isWeakToolTitle("Skill")).toBe(true);
    expect(isWeakToolTitle("Skill /code-review")).toBe(false);
  });
});

describe("agent titles", () => {
  it("prefers the description, then the subagent type", () => {
    expect(
      titleFromToolInput("Agent", "agent", {
        description: "Explore the auth module",
        subagent_type: "explore",
      }),
    ).toBe("Explore the auth module");
    expect(titleFromToolInput("Task", "agent", { subagent_type: "explore" })).toBe(
      "Explore subagent",
    );
    expect(titleFromToolInput("Agent", "agent", {})).toBe("Subagent");
  });

  it("labels a composed agent kind as Subagent when the title is a placeholder", () => {
    expect(composeToolTitle({ kind: "agent", title: "Task" })).toBe("Subagent");
    expect(
      composeToolTitle({ kind: "agent", title: "Explore the auth module" }),
    ).toBe("Explore the auth module");
  });
});
