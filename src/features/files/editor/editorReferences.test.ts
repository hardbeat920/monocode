import { EditorState, Text } from "@codemirror/state";
import { expect, it } from "vitest";
import {
  editorReferences,
  locateReference,
  referenceAt,
  setEditorReferences,
} from "./editorReferences";

function start(doc: string) {
  return EditorState.create({ doc, extensions: editorReferences(() => {}) });
}

it("finds a cited passage by text when the saved offsets are stale", () => {
  const doc = Text.of(["intro", "the cited line", "outro"]);
  expect(
    locateReference(doc, { id: "a", text: "the cited line", from: 0, to: 14 }),
  ).toEqual({ from: 6, to: 20 });
  expect(
    locateReference(doc, { id: "a", text: "gone", from: 0, to: 4 }),
  ).toBeNull();
});

it("marks cited passages and moves the mark with edits above it", () => {
  let state = start("one\ntwo\nthree");
  const mark = { id: "a", text: "two", from: 4, to: 7 };
  state = state.update({ effects: setEditorReferences.of([mark]) }).state;
  expect(referenceAt(state, 5)).toEqual({ id: "a", from: 4, to: 7 });
  expect(referenceAt(state, 10)).toBeNull();

  state = state.update({ changes: { from: 0, insert: "zero\n" } }).state;
  expect(referenceAt(state, 9)).toEqual({ id: "a", from: 9, to: 12 });

  // Re-sending the list keeps the tracked position, and drops removed ones.
  state = state.update({ effects: setEditorReferences.of([mark]) }).state;
  expect(referenceAt(state, 9)).toEqual({ id: "a", from: 9, to: 12 });
  state = state.update({ effects: setEditorReferences.of([]) }).state;
  expect(referenceAt(state, 9)).toBeNull();
});
