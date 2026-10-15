import {
  RangeSetBuilder,
  StateEffect,
  StateField,
  type EditorState,
  type Extension,
  type Text,
} from "@codemirror/state";
import { Decoration, EditorView, type DecorationSet } from "@codemirror/view";

/** A cited passage and where it was when cited; the field tracks it from there. */
export type EditorReferenceMark = {
  id: string;
  text: string;
  from: number;
  to: number;
};

/** Replaces the cited passages marked in the editor. */
export const setEditorReferences = StateEffect.define<EditorReferenceMark[]>();

/**
 * Where a cited passage sits now. The offsets from citing time may be stale if
 * the file changed while it was closed, so fall back to finding the text.
 */
export function locateReference(
  doc: Text,
  mark: EditorReferenceMark,
): { from: number; to: number } | null {
  if (
    mark.to <= doc.length &&
    mark.from < mark.to &&
    doc.sliceString(mark.from, mark.to) === mark.text
  ) {
    return { from: mark.from, to: mark.to };
  }
  const found = mark.text ? doc.toString().indexOf(mark.text) : -1;
  return found < 0 ? null : { from: found, to: found + mark.text.length };
}

function idOf(value: Decoration): string {
  return (value.spec as { chatReferenceId: string }).chatReferenceId;
}

const marks = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(value, tr) {
    let next = value.map(tr.changes);
    for (const effect of tr.effects) {
      if (!effect.is(setEditorReferences)) continue;
      // Keep the tracked position of a passage that is still cited; place a
      // new one from where it was cited.
      const tracked = new Map<string, { from: number; to: number }>();
      next.between(0, tr.newDoc.length, (from, to, deco) => {
        tracked.set(idOf(deco), { from, to });
      });
      const placed = effect.value.flatMap((mark) => {
        const at = tracked.get(mark.id) ?? locateReference(tr.newDoc, mark);
        return at && at.from < at.to
          ? [
              {
                at,
                deco: Decoration.mark({
                  class: "cm-chat-reference",
                  chatReferenceId: mark.id,
                }),
              },
            ]
          : [];
      });
      placed.sort((a, b) => a.at.from - b.at.from || a.at.to - b.at.to);
      const builder = new RangeSetBuilder<Decoration>();
      for (const { at, deco } of placed) builder.add(at.from, at.to, deco);
      next = builder.finish();
    }
    return next;
  },
  provide: (field) => EditorView.decorations.from(field),
});

/** The cited passage covering `pos`, with its current range. */
export function referenceAt(
  state: EditorState,
  pos: number,
): { id: string; from: number; to: number } | null {
  let hit: { id: string; from: number; to: number } | null = null;
  state.field(marks).between(pos, pos, (from, to, deco) => {
    if (from <= pos && pos <= to) {
      hit = { id: idOf(deco), from, to };
      return false;
    }
  });
  return hit;
}

/** Marks cited passages and reports a plain click on one. */
export function editorReferences(
  onOpen: (reference: { id: string; from: number; to: number }) => void,
): Extension {
  return [
    marks,
    EditorView.domEventHandlers({
      click(event, view) {
        if (!view.state.selection.main.empty) return false;
        const pos = view.posAtCoords({ x: event.clientX, y: event.clientY });
        const hit = pos == null ? null : referenceAt(view.state, pos);
        if (hit) onOpen(hit);
        return false;
      },
    }),
  ];
}
