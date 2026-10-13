// Hit test (spec 9.2 step 6). `this` is the target element in the
// "__monocode" isolated world; `x`, `y` are viewport coordinates of the
// target's own frame.
//
// Without `other`: elementFromPoint, descending through open shadow roots.
// The hit is good when it is the target or inside it. A hit on a shadow host
// that (in the composed tree) contains the target means a closed shadow root
// hides the real hit: the result is "unknown" and the Rust side asks
// DOM.getNodeForLocation, which sees closed roots, then calls this function
// again with that node as `other`.
//
// Read-only: nothing on the page changes.
function monocodeHitTest(x, y, other) {
  const target = this;
  const up = (n) => (n.parentNode ? n.parentNode : n.host ? n.host : null);
  const inside = (hit) => {
    for (let n = hit; n; n = up(n)) {
      if (n === target) {
        return true;
      }
    }
    return false;
  };
  const describe = (e) => {
    if (!e || e.nodeType !== 1) {
      return "";
    }
    let label = e.tagName.toLowerCase();
    if (e.id) {
      label += "#" + e.id;
    }
    const text = String(e.innerText || e.textContent || "")
      .replace(/\s+/g, " ")
      .trim()
      .slice(0, 60);
    return text ? label + ' "' + text + '"' : label;
  };
  if (other) {
    const hit = other.nodeType === 1 ? other : other.parentElement;
    return inside(other) ? { status: "ok" } : { status: "obscured", by: describe(hit) };
  }
  const doc = target.ownerDocument;
  let hit = doc.elementFromPoint(x, y);
  while (hit && hit.shadowRoot) {
    const inner = hit.shadowRoot.elementFromPoint(x, y);
    if (!inner || inner === hit) {
      break;
    }
    hit = inner;
  }
  if (!hit) {
    return { status: "unknown" };
  }
  if (inside(hit)) {
    return { status: "ok" };
  }
  // The hit is an ancestor of the target in the composed tree: the target
  // lives in a closed shadow root under it.
  for (let n = up(target); n; n = up(n)) {
    if (n === hit) {
      return { status: "unknown" };
    }
  }
  return { status: "obscured", by: describe(hit) };
}
