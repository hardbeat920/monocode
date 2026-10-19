// @vitest-environment happy-dom
import { foldGutter } from "@codemirror/language";
import {
  EditorState,
  type StateEffect,
  type TransactionSpec,
} from "@codemirror/state";
import { EditorView, lineNumbers } from "@codemirror/view";
import { afterEach, describe, expect, it, vi } from "vitest";
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

describe("change overview", () => {
  it("scrolls to a clicked tick's change, not the line under the pointer", async () => {
    const dom = mount(
      "alpha\nbeta\nGAMMA\ndelta\n",
      "alpha\nbeta\ngamma\ndelta\n",
    );
    const editor = view!;
    await new Promise((resolve) => requestAnimationFrame(resolve));
    const tick = dom.querySelector<HTMLElement>(".cm-gitOverviewTick");
    expect(tick).not.toBeNull();

    // Ticks are at least 3px tall, so a click near a tick's bottom edge can
    // sit over a later line on the rail's scale.
    const rail = dom.querySelector<HTMLElement>(".cm-gitOverview")!;
    rail.getBoundingClientRect = () => new DOMRect(0, 0, 18, 100);
    const dispatch = vi.spyOn(editor, "dispatch");
    tick!.dispatchEvent(
      new MouseEvent("mousedown", { bubbles: true, button: 0, clientY: 99 }),
    );
    // Dragging on from the tick follows the pointer down the rail.
    window.dispatchEvent(new MouseEvent("mousemove", { clientY: 99 }));
    window.dispatchEvent(new MouseEvent("mouseup"));

    const targets = dispatch.mock.calls.map(([spec]) => {
      const effect = (spec as TransactionSpec).effects as StateEffect<{
        range: { head: number };
      }>;
      return effect.value.range.head;
    });
    const { doc } = editor.state;
    expect(targets).toEqual([doc.line(3).from, doc.line(doc.lines).from]);
  });
});
