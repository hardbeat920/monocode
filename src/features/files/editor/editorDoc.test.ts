import { EditorState } from "@codemirror/state";
import { describe, expect, it } from "vitest";
import {
  detectLineEnding,
  editorDocChanges,
  normalizeLineBreaks,
  restoreLineEnding,
} from "./editorDoc";

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

  it("treats CRLF disk content as identical to the LF document", () => {
    expect(editorDocChanges("alpha\nbeta\n", "alpha\r\nbeta\r\n")).toEqual([]);
  });

  it("never doubles lines when the target uses CRLF", () => {
    const from = "alpha\nbeta\ngamma\n";
    const to = "alpha\r\nBETA\r\ngamma\r\n";
    const state = EditorState.create({ doc: from });
    const next = state.update({ changes: editorDocChanges(from, to) }).state;
    // Pre-fix this produced "alpha\n\nBETA\n\ngamma\n\n": the lone "\r"
    // inserts were converted into line breaks by CodeMirror.
    expect(next.doc.toString()).toBe("alpha\nBETA\ngamma\n");
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

describe("line-ending round trip", () => {
  it.each(["alpha\nbeta\n", "alpha\r\nbeta\r\n", "alpha\rbeta\r"])(
    "load then save leaves %j byte-identical",
    (raw) => {
      // The FileEditor load/save contract: normalize into the LF document,
      // restore the file's own convention on write.
      const restored = restoreLineEnding(
        normalizeLineBreaks(raw),
        detectLineEnding(raw),
      );
      expect(restored).toBe(raw);
    },
  );
});
