/**
 * Putting back text a paste handler withheld from the webview. The native
 * clipboard read crosses an IPC hop, so a draft can be typed into or text
 * selected by the time the paste resolves; the captured range is reused only
 * while the draft is still the one that was captured.
 */

type DraftField = HTMLTextAreaElement | HTMLInputElement;

export type CapturedDraft = {
  field: DraftField;
  value: string;
  start: number;
  end: number;
};

/** The field a paste landed in, with its value and selection, or null. */
export function captureDraft(
  target: EventTarget | null,
): CapturedDraft | null {
  const field =
    target instanceof HTMLTextAreaElement ||
    target instanceof HTMLInputElement
      ? target
      : null;
  if (!field) return null;
  return {
    field,
    value: field.value,
    start: field.selectionStart ?? 0,
    end: field.selectionEnd ?? 0,
  };
}

/** Insert at the captured range, or at the caret if the draft has moved on. */
export function insertRestoredText(captured: CapturedDraft, text: string) {
  const { field } = captured;
  const unchanged = field.value === captured.value;
  const start = unchanged
    ? captured.start
    : (field.selectionStart ?? field.value.length);
  const end = unchanged ? captured.end : start;
  field.setRangeText(text, start, end, "end");
  // A raw `input` event is what React listens for.
  field.dispatchEvent(new Event("input", { bubbles: true }));
}
