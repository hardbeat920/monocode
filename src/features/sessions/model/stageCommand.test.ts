import { describe, expect, it } from "vitest";
import { skillNamesInText } from "../../skills/model/skills";
import type { Block } from "./session";
import {
  consumeStageCommand,
  isStageCommand,
  sessionChangedFiles,
  stageTurnPrompt,
} from "./stageCommand";

describe("stage command", () => {
  it("matches a standalone /stage command", () => {
    expect(isStageCommand("/stage")).toBe(true);
    expect(isStageCommand("  /STAGE\n")).toBe(true);
    expect(isStageCommand("/stage only docs")).toBe(false);
    expect(isStageCommand("/staged")).toBe(false);
  });

  it("consumes the leading command and keeps extra instructions", () => {
    expect(consumeStageCommand("/stage")).toEqual({ text: "", matched: true });
    expect(consumeStageCommand("/stage  only the docs \n")).toEqual({
      text: "only the docs",
      matched: true,
    });
    expect(consumeStageCommand("please /stage")).toEqual({
      text: "please /stage",
      matched: false,
    });
    expect(consumeStageCommand("/stages")).toEqual({
      text: "/stages",
      matched: false,
    });
  });
});

describe("sessionChangedFiles", () => {
  it("collects successful edits from every turn, relative to the project", () => {
    const blocks: Block[] = [
      { id: "u1", role: "user", text: "Fix the parser" },
      {
        id: "edit-1",
        role: "tool",
        text: "Edit",
        tool: {
          kind: "edit",
          status: "completed",
          paths: ["/repo/src/parser.ts"],
          preview: { kind: "diff", path: "/repo/src/parser.ts" },
        },
      },
      {
        id: "read",
        role: "tool",
        text: "Read",
        tool: {
          kind: "read",
          status: "completed",
          preview: { kind: "read", path: "/repo/src/other.ts" },
        },
      },
      {
        id: "failed",
        role: "tool",
        text: "Edit",
        tool: {
          kind: "edit",
          status: "failed",
          preview: { kind: "write", path: "/repo/failed.ts" },
        },
      },
      { id: "u2", role: "user", text: "Add docs" },
      {
        id: "edit-2",
        role: "tool",
        text: "Write",
        tool: {
          kind: "edit",
          status: "success",
          preview: { kind: "write", path: "docs/parser.md" },
        },
      },
    ];

    expect(sessionChangedFiles(blocks, "/repo")).toEqual([
      "src/parser.ts",
      "docs/parser.md",
    ]);
  });
});

describe("stageTurnPrompt", () => {
  it("asks for path-scoped staging of the listed files and the user's extra request", () => {
    const prompt = stageTurnPrompt("skip the lockfile", [
      "src/parser.ts",
      "docs/parser.md",
    ]);

    expect(prompt).toContain("Stage the changes you made in this session");
    expect(prompt).toContain("do not use `git add .`");
    expect(prompt).toContain("Do not commit");
    expect(prompt).toContain("- src/parser.ts\n- docs/parser.md");
    expect(prompt.endsWith("skip the lockfile")).toBe(true);
  });

  it("leaves out empty sections", () => {
    const prompt = stageTurnPrompt("  ", []);

    expect(prompt).not.toContain("Files MonoCode recorded");
    expect(prompt).not.toContain("Additional instructions");
  });

  it("does not invoke skills or native commands by itself", () => {
    const prompt = stageTurnPrompt("", ["src/a.ts"]);

    expect(skillNamesInText(prompt)).toEqual([]);
    expect(prompt.startsWith("/")).toBe(false);
  });
});
