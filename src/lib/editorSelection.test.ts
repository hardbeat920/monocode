import { describe, expect, it } from "vitest";
import { formatEditorSelectionReference } from "./editorSelection";

describe("formatEditorSelectionReference", () => {
  it("includes a mentionable file and line range without copying its contents", () => {
    expect(
      formatEditorSelectionReference({
        path: "src/FileEditor.tsx",
        startLine: 12,
        endLine: 13,
      }),
    ).toBe("@src/FileEditor.tsx (lines 12-13)");
  });

  it("uses a compact location for a single selected line", () => {
    expect(
      formatEditorSelectionReference({
        path: "src/value.ts",
        startLine: 7,
        endLine: 7,
      }),
    ).toBe("@src/value.ts (line 7)");
  });

  it("quotes paths that cannot be represented as composer mentions", () => {
    expect(
      formatEditorSelectionReference({
        path: "docs/read me.md",
        startLine: 3,
        endLine: 5,
      }),
    ).toBe("`docs/read me.md` (lines 3-5)");
  });
});
