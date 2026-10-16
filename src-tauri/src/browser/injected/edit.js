// `type {clear: true}`: select the whole content of the focused
// editable element so the following Input.insertText replaces it. `this` is
// the element (or a text node, from a full snapshot, inside it), in the
// "__monocode" isolated world. Selecting is the only
// change; the text itself always arrives through Input.insertText, so the
// page sees ordinary input events.
function monocodeSelectAll() {
  let el = this && this.nodeType !== 1 ? this.parentElement : this;
  if (el && el !== this && el.isContentEditable) {
    // Text inside an editor: the whole editing host is replaced.
    while (el.parentElement && el.parentElement.isContentEditable) {
      el = el.parentElement;
    }
  }
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
