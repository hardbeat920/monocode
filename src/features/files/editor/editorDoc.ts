import { diff } from "@codemirror/merge";
import type { Annotation, ChangeSpec } from "@codemirror/state";
import { EditorView } from "@codemirror/view";

const DOC_DIFF = { scanLimit: 5_000, timeout: 100 };

export function editorDocChanges(from: string, to: string): ChangeSpec[] {
  if (from === to) return [];
  return diff(from, to, DOC_DIFF).map((change) => ({
    from: change.fromA,
    to: change.toA,
    insert: to.slice(change.fromB, change.toB),
  }));
}

export function replaceEditorDoc(
  view: EditorView,
  next: string,
  options?: {
    selection?: { anchor: number; head?: number };
    annotations?: readonly Annotation<unknown>[];
  },
): boolean {
  const prev = view.state.doc.toString();
  if (prev === next) return false;
  const changes = editorDocChanges(prev, next);
  view.dispatch({
    changes:
      changes.length > 0
        ? changes
        : { from: 0, to: view.state.doc.length, insert: next },
    selection: options?.selection,
    annotations: options?.annotations,
    scrollIntoView: false,
    effects: view.scrollSnapshot(),
  });
  return true;
}

/** Keep the caret at the same screen position across a layout-changing update. */
export function preserveEditorViewport(view: EditorView, mutate: () => void) {
  const head = view.state.selection.main.head;
  const before = view.coordsAtPos(head);
  const scroller = view.scrollDOM;
  const scrollerTop = scroller.getBoundingClientRect().top;
  const offsetY = before ? before.top - scrollerTop : null;
  const scrollTop = scroller.scrollTop;
  const scrollLeft = scroller.scrollLeft;

  mutate();

  const restore = () => {
    if (!view.dom.isConnected) return;
    if (offsetY == null) {
      scroller.scrollTop = scrollTop;
      scroller.scrollLeft = scrollLeft;
      return;
    }
    const after = view.coordsAtPos(view.state.selection.main.head);
    if (!after) return;
    scroller.scrollTop +=
      after.top - scroller.getBoundingClientRect().top - offsetY;
  };
  restore();
  view.requestMeasure({
    key: preserveEditorViewport,
    read: () => true,
    write: restore,
  });
}
