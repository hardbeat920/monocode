// @vitest-environment happy-dom
import { EditorView } from "@codemirror/view";
import { afterEach, describe, expect, it } from "vitest";
import { stateWithGitOriginal } from "./editorGit";

let view: EditorView | null = null;

afterEach(() => {
  view?.destroy();
  view = null;
});

function mount(doc: string, original: string) {
  const parent = document.createElement("div");
  document.body.append(parent);
  view = new EditorView({ state: stateWithGitOriginal(doc, original), parent });
  return view.dom;
}

describe("git decorations", () => {
  it("marks every removed and added line with a glyph, not only color", () => {
    const dom = mount(
      "alpha\nBETA\nepsilon\n",
      "alpha\nbeta\ngamma\ndelta\nepsilon\n",
    );

    const deleted = [
      ...dom.querySelectorAll<HTMLElement>(".cm-gitDeletedLine"),
    ];
    expect(deleted.map((line) => line.textContent)).toEqual([
      "beta",
      "gamma",
      "delta",
    ]);
    expect(deleted.map((line) => line.dataset.oldLine)).toEqual([
      "2",
      "3",
      "4",
    ]);

    const delMarks = [...dom.querySelectorAll(".cm-gitGutter .cm-gitDel")];
    expect(delMarks.map((mark) => mark.textContent)).toEqual(["−", "−", "−"]);

    const addMarks = [...dom.querySelectorAll(".cm-gitGutter .cm-gitAdd")];
    expect(addMarks.map((mark) => mark.textContent)).toEqual(["+"]);
    expect(dom.querySelectorAll(".cm-gitInsertedLine")).toHaveLength(1);
  });
});
