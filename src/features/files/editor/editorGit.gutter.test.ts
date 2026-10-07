// @vitest-environment happy-dom
import { foldGutter } from "@codemirror/language";
import { EditorState } from "@codemirror/state";
import { EditorView, lineNumbers } from "@codemirror/view";
import { afterEach, describe, expect, it } from "vitest";
import { editorGit, setGitOriginal, stateWithGitOriginal } from "./editorGit";

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

  it("puts the glyph lane next to the code and tints the whole gutter row", () => {
    const parent = document.createElement("div");
    document.body.append(parent);
    // Same order as FileEditor's diff tabs: git extension first.
    view = new EditorView({
      state: EditorState.create({
        doc: "alpha\nBETA\nepsilon\n",
        extensions: [editorGit(), foldGutter(), lineNumbers()],
      }),
      parent,
    });
    setGitOriginal(view, "alpha\nbeta\ngamma\nepsilon\n");
    const dom = view.dom;

    const gutters = [...dom.querySelectorAll(".cm-gutters > .cm-gutter")];
    expect(gutters.map((el) => el.classList[1])).toEqual([
      "cm-foldGutter",
      "cm-lineNumbers",
      "cm-gitGutter",
    ]);

    const numbers = dom.querySelector(".cm-lineNumbers")!;
    expect(
      [...numbers.querySelectorAll(".cm-gitDelRow")].map(
        (el) => el.textContent,
      ),
    ).toEqual(["2", "3"]);
    expect(
      [...numbers.querySelectorAll(".cm-gitAddRow")].map(
        (el) => el.textContent,
      ),
    ).toEqual(["2"]);
    expect(
      dom.querySelectorAll(".cm-gitGutter .cm-gitAddRow .cm-gitAdd"),
    ).toHaveLength(1);
    expect(
      dom.querySelectorAll(".cm-gitGutter .cm-gitDelRow .cm-gitDel"),
    ).toHaveLength(2);
  });
});
