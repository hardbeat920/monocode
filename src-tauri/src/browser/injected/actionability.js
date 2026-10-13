// Actionability check (spec 9.2 step 3), one call per element.
//
// Called with Runtime.callFunctionOn on the element's object in the
// "__monocode" isolated world, `this` being the element. Reads layout and
// state only; never changes the page. Returns everything the Rust side needs
// in one structure so a failed check costs a single round trip.
//
// mode "rect" returns just the bounding box (stability check, spec 9.2:
// the same box in two samples taken a short while apart).
function monocodeActionability(mode) {
  const node = this;
  const element =
    node && node.nodeType === 1 ? node : node && node.parentElement ? node.parentElement : null;
  const box = (e) => {
    const r = e.getBoundingClientRect();
    return { x: r.x, y: r.y, width: r.width, height: r.height };
  };
  if (!node || !node.isConnected || !element) {
    return { connected: false };
  }
  if (mode === "rect") {
    return { connected: true, rect: box(element) };
  }
  const style = element.ownerDocument.defaultView.getComputedStyle(element);
  const rect = box(element);
  let visible = rect.width > 0 && rect.height > 0 && style.visibility !== "hidden";
  if (visible && typeof element.checkVisibility === "function") {
    // `opacity: 0` still counts as visible, like Playwright (spec 9.2).
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
