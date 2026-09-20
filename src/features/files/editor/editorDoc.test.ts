import { EditorState } from "@codemirror/state";
import { describe, expect, it } from "vitest";
import { editorDocChanges } from "./editorDoc";

describe("editorDocChanges", () => {
  it("returns no changes when the documents match", () => {
    expect(editorDocChanges("alpha\nbeta\n", "alpha\nbeta\n")).toEqual([]);
  });

  it("rewrites only the edited span so later matches keep their offsets", () => {
    const from = "const hello = 1;\nconst hello = 2;\n";
    const to = "const hello = 1;\nconst hallo = 2;\n";
    expect(editorDocChanges(from, to)).toEqual([
      { from: 24, to: 25, insert: "a" },
    ]);
  });

  it("maps a trailing-newline format as a small tail insert", () => {
    const from = "const hello = 1;";
    const to = "const hello = 1;\n";
    const changes = editorDocChanges(from, to);
    expect(changes).toEqual([{ from: 16, to: 16, insert: "\n" }]);
  });

  it("keeps a later search selection on the same match after an earlier edit", () => {
    const from = "const hello = 1;\nconst hello = 2;\n";
    const to = "const hallo = 1;\nconst hello = 2;\n";
    const second = from.indexOf("hello", from.indexOf("hello") + 1);
    const state = EditorState.create({
      doc: from,
      selection: { anchor: second, head: second + 5 },
    });
    const next = state.update({ changes: editorDocChanges(from, to) }).state;
    expect(
      next.sliceDoc(next.selection.main.from, next.selection.main.to),
    ).toBe("hello");
    expect(next.doc.lineAt(next.selection.main.from).number).toBe(2);
  });
});
