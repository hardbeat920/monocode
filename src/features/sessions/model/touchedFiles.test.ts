import { describe, expect, it } from "vitest";
import type { Block } from "./session";
import {
  resolveSessionFileReference,
  sessionTouchedPaths,
} from "./touchedFiles";

function tool(
  id: string,
  preview: NonNullable<Block["tool"]>["preview"],
  extra: Partial<NonNullable<Block["tool"]>> = {},
): Block {
  return {
    id,
    role: "tool",
    text: id,
    tool: { kind: "read", status: "completed", preview, ...extra },
  };
}

const NOTES = "/Users/me/agents/sales/notes/NOTES.md";

describe("sessionTouchedPaths", () => {
  it("collects the absolute paths of tool calls and skips the rest", () => {
    const blocks: Block[] = [
      { id: "user", role: "user", text: `Update ${NOTES}` },
      tool("read", { kind: "read", path: NOTES }),
      // A shell tool recorded only the name it was given.
      tool("shell", { kind: "write", path: "NOTES.md" }, { kind: "shell" }),
      tool("again", { kind: "read", path: NOTES }),
      tool("multi", undefined, { paths: ["/repo/a.ts", "/repo/b.ts"] }),
      tool(
        "failed",
        { kind: "read", path: "/repo/gone.ts" },
        { status: "failed" },
      ),
    ];
    expect(sessionTouchedPaths(blocks)).toEqual([
      NOTES,
      "/repo/a.ts",
      "/repo/b.ts",
    ]);
  });
});

describe("resolveSessionFileReference", () => {
  const cwd = "/Users/me";

  it("opens a bare name from the one touched file that has it", () => {
    expect(resolveSessionFileReference("NOTES.md", cwd, [NOTES])).toEqual({
      path: NOTES,
    });
    expect(
      resolveSessionFileReference("./NOTES.md:12", cwd, ["/repo/a.ts", NOTES]),
    ).toEqual({ path: NOTES, navigation: { line: 12 } });
  });

  it("leaves a bare name alone when several touched files have it", () => {
    const other = "/Users/me/agents/marketing/notes/NOTES.md";
    expect(
      resolveSessionFileReference("NOTES.md", cwd, [NOTES, other]),
    ).toEqual({ path: "/Users/me/NOTES.md" });
  });

  it("leaves a name alone when no touched file has it", () => {
    expect(
      resolveSessionFileReference("NOTES.md", cwd, ["/repo/a.ts"]),
    ).toEqual({ path: "/Users/me/NOTES.md" });
    expect(resolveSessionFileReference("NOTES.md", cwd, [])).toEqual({
      path: "/Users/me/NOTES.md",
    });
  });

  it("leaves an absolute reference alone", () => {
    expect(
      resolveSessionFileReference("/Users/me/NOTES.md", cwd, [NOTES]),
    ).toEqual({ path: "/Users/me/NOTES.md" });
    expect(resolveSessionFileReference("~/NOTES.md", cwd, [NOTES])).toEqual({
      path: "/Users/me/NOTES.md",
    });
    expect(
      resolveSessionFileReference("file:///Users/me/NOTES.md", cwd, [
        NOTES,
      ]),
    ).toEqual({ path: "/Users/me/NOTES.md" });
  });

  it("matches a relative path by its end", () => {
    expect(
      resolveSessionFileReference("notes/NOTES.md", cwd, [NOTES]),
    ).toEqual({ path: NOTES });
    expect(
      resolveSessionFileReference("sales/NOTES.md", cwd, [NOTES]),
    ).toEqual({ path: "/Users/me/sales/NOTES.md" });
  });

  it("ignores case only on Windows paths", () => {
    const windows = "C:/Users/me/agents/sales/notes/NOTES.md";
    expect(
      resolveSessionFileReference("notes.md", "C:/Users/me", [windows]),
    ).toEqual({ path: windows });
    expect(resolveSessionFileReference("notes.md", cwd, [NOTES])).toEqual({
      path: "/Users/me/notes.md",
    });
  });

  it("matches whole path segments only", () => {
    expect(resolveSessionFileReference("OTES.md", cwd, [NOTES])).toEqual({
      path: "/Users/me/OTES.md",
    });
  });

  it("does not use local paths for a connected machine", () => {
    expect(
      resolveSessionFileReference("NOTES.md", "remote://box/home/g", [NOTES]),
    ).toEqual({ path: "remote://box/home/g/NOTES.md" });
  });

  it("still returns nothing for text that is not a file reference", () => {
    expect(resolveSessionFileReference("true", cwd, [NOTES])).toBeUndefined();
  });
});
