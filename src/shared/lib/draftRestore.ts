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

/** What the draft would be if `text` were pasted at the captured range. */
function valueWithPaste(captured: CapturedDraft, text: string): string {
  return (
    captured.value.slice(0, captured.start) +
    text +
    captured.value.slice(captured.end)
  );
}

/**
 * Take a withheld paste back out of the field.
 *
 * A file URI is kept out of the draft with `preventDefault`, but WebKit can
 * still insert it. Once that paste has become an attachment, the URI has to
 * leave; anything typed after it stays.
 */
export function dropPastedText(captured: CapturedDraft, text: string) {
  if (!text) return;
  const { field } = captured;
  const inserted = valueWithPaste(captured, text);
  if (field.value.startsWith(inserted)) {
    const extra = field.value.slice(inserted.length);
    const restored = captured.value + extra;
    if (field.value === restored) return;
    field.value = restored;
    const caret = extra ? restored.length : captured.start;
    field.setSelectionRange(caret, caret);
    field.dispatchEvent(new Event("input", { bubbles: true }));
    return;
  }
  // Inserted before this handler observed the field, so the capture already
  // contains it. A paste leaves the caret at the end of that text.
  const end = field.selectionEnd ?? field.value.length;
  const start = end - text.length;
  if (start < 0 || field.value.slice(start, end) !== text) return;
  field.setRangeText("", start, end, "end");
  field.dispatchEvent(new Event("input", { bubbles: true }));
}

/** Insert at the captured range, or at the caret if the draft has moved on. */
export function insertRestoredText(captured: CapturedDraft, text: string) {
  const { field } = captured;
  // The webview already landed this paste. Putting it in again would double it.
  if (text && field.value.startsWith(valueWithPaste(captured, text))) return;
  const unchanged = field.value === captured.value;
  const start = unchanged
    ? captured.start
    : (field.selectionStart ?? field.value.length);
  const end = unchanged ? captured.end : start;
  field.setRangeText(text, start, end, "end");
  // A raw `input` event is what React listens for.
  field.dispatchEvent(new Event("input", { bubbles: true }));
}
