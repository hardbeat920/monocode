// Document-level helpers for `wait`, `scroll` and the post-action
// observation (spec 9.2 step 9, 9.4). Called with Runtime.callFunctionOn
// against the "__monocode" isolated world's execution context of a frame.
//
// kind:
//   "text"      -> { found }: whether the visible text contains `value`
//   "ready"     -> { state }: document.readyState
//   "mutations" -> { count }: DOM mutation records since the previous call
//                  (the observer is installed on first use and lives in the
//                  isolated world, invisible to the page)
//   "scroll"    -> { x, y }: window scroll position; with numbers in `value`
//                  ({dx, dy}) scrolls by that much first (wheel fallback)
function monocodePage(kind, value) {
  const doc = document;
  if (kind === "text") {
    const root = doc.body || doc.documentElement;
    const text = root ? String(root.innerText || root.textContent || "") : "";
    const needle = String(value);
    if (text.includes(needle)) {
      return { found: true };
    }
    const squash = (s) => s.replace(/\s+/g, " ");
    return { found: squash(text).includes(squash(needle)) };
  }
  if (kind === "ready") {
    return { state: doc.readyState };
  }
  if (kind === "mutations") {
    const key = "__monocodeMutations";
    let state = window[key];
    if (!state) {
      state = { count: 0, observer: null };
      state.observer = new MutationObserver((records) => {
        state.count += records.length;
      });
      state.observer.observe(doc, {
        subtree: true,
        childList: true,
        attributes: true,
        characterData: true,
      });
      window[key] = state;
    }
    state.count += state.observer.takeRecords().length;
    const count = state.count;
    state.count = 0;
    return { count };
  }
  if (kind === "scroll") {
    if (value && (value.dx || value.dy)) {
      window.scrollBy(value.dx || 0, value.dy || 0);
    }
    return { x: window.scrollX, y: window.scrollY };
  }
  return null;
}
