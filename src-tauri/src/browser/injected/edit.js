// `type {clear: true}` (spec 9.3): select the whole content of the focused
// editable element so the following Input.insertText replaces it. `this` is
// the element, in the "__monocode" isolated world. Selecting is the only
// change; the text itself always arrives through Input.insertText, so the
// page sees ordinary input events.
function monocodeSelectAll() {
  const el = this;
  if (!el || !el.isConnected) {
    return { ok: false };
  }
  el.focus();
  const tag = el.tagName ? el.tagName.toLowerCase() : "";
  if (tag === "input" || tag === "textarea") {
    if (typeof el.select === "function") {
      el.select();
    }
    return { ok: true, length: String(el.value || "").length };
  }
  if (el.isContentEditable) {
    const selection = el.ownerDocument.getSelection();
    const range = el.ownerDocument.createRange();
    range.selectNodeContents(el);
    selection.removeAllRanges();
    selection.addRange(range);
    return { ok: true, length: String(el.textContent || "").length };
  }
  return { ok: false };
}
