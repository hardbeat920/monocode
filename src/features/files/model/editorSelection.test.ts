import { describe, expect, it } from "vitest";
import { editorSelectionContext } from "./editorSelection";

describe("editorSelectionContext", () => {
  it("references the file and line range without copying its contents", () => {
    expect(
      editorSelectionContext({
        path: "src/FileEditor.tsx",
        startLine: 12,
        endLine: 13,
      }),
    ).toEqual({
      kind: "code",
      path: "src/FileEditor.tsx",
      startLine: 12,
      endLine: 13,
    });
  });

  it("uses forward slashes for Windows paths", () => {
    expect(
      editorSelectionContext({
        path: "docs\\read me.md",
        startLine: 3,
        endLine: 3,
      }),
    ).toMatchObject({ path: "docs/read me.md" });
  });
});
