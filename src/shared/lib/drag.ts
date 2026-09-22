export const EXPLORER_FILE_POINTER_DRAG_EVENT =
  "monocode:explorer-file-pointer-drag";

export type ExplorerFilePointerDragDetail =
  | { type: "move" | "drop"; path: string; x: number; y: number }
  | { type: "end"; path: string };

export function emitExplorerFilePointerDrag(
  detail: ExplorerFilePointerDragDetail,
) {
  window.dispatchEvent(
    new CustomEvent<ExplorerFilePointerDragDetail>(
      EXPLORER_FILE_POINTER_DRAG_EVENT,
      { detail },
    ),
  );
}

export function setGrabbing(on: boolean) {
  document.body.style.cursor = on ? "grabbing" : "";
}

/** Block native text selection for the duration of a reorder gesture. */
export function suppressTextSelection() {
  const onSelectStart = (event: Event) => {
    event.preventDefault();
  };
  window.addEventListener("selectstart", onSelectStart);
  document.documentElement.classList.add("is-reordering");
  window.getSelection()?.removeAllRanges();
  return () => {
    window.removeEventListener("selectstart", onSelectStart);
    document.documentElement.classList.remove("is-reordering");
    window.getSelection()?.removeAllRanges();
  };
}
