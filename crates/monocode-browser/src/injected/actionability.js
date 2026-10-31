// Actionability check, one call per element.
//
// Called with Runtime.callFunctionOn on the element's object in the
// "__monocode" isolated world, `this` being the element (or a text node, which
// `text` reports). Reads layout and
// state only; never changes the page. Returns everything the Rust side needs
// in one structure so a failed check costs a single round trip.
//
// mode "rect" returns just the bounding box (stability check:
// the same box in two samples taken a short while apart).
function monocodeActionability(mode) {
  const node = this;
  const element =
    node && node.nodeType === 1 ? node : node && node.parentElement ? node.parentElement : null;
  const box = (e) => {
    const r = e.getBoundingClientRect();
    return { x: r.x, y: r.y, width: r.width, height: r.height };
  };
  // A text node (a full snapshot's static text) is measured by its own
  // line boxes; every state check uses its element.
  const nodeBox = () => {
    if (node === element) {
      return box(element);
    }
    const range = node.ownerDocument.createRange();
    range.selectNodeContents(node);
    return box(range);
  };
  if (!node || !node.isConnected || !element) {
    return { connected: false };
  }
  if (mode === "rect") {
    return { connected: true, rect: nodeBox() };
  }
  const style = element.ownerDocument.defaultView.getComputedStyle(element);
  const rect = nodeBox();
  let visible = rect.width > 0 && rect.height > 0 && style.visibility !== "hidden";
  if (visible && typeof element.checkVisibility === "function") {
    // `opacity: 0` still counts as visible, like Playwright.
    visible = element.checkVisibility({ visibilityProperty: true });
  }
  const tag = element.tagName.toLowerCase();
  const type = tag === "input" ? String(element.type || "text").toLowerCase() : "";
  const disabled =
    (typeof element.matches === "function" && element.matches(":disabled")) ||
    !!element.closest('[aria-disabled="true"]');
  const nonText = [
    "button",
    "checkbox",
    "color",
    "file",
    "hidden",
    "image",
    "radio",
    "range",
    "reset",
    "submit",
  ];
  const textLike = (tag === "input" && !nonText.includes(type)) || tag === "textarea";
  const editable = (textLike && !element.readOnly) || element.isContentEditable === true;
  let label = tag;
  if (element.id) {
    label += "#" + element.id;
  }
  return {
    connected: true,
    text: node !== element,
    visible,
    enabled: !disabled,
    editable,
    text_like: textLike,
    file_input: tag === "input" && type === "file",
    multiple: !!element.multiple,
    select: tag === "select",
    tag: label,
    rect,
  };
}
