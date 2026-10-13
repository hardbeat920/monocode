// `select` (spec 9.4). `this` is a <select> element in the "__monocode"
// isolated world; `values` are matched by option value first, then by label.
//
// Every requested value is checked before anything changes, so a missing
// value leaves the element untouched. On success the options are set and
// `input` and `change` are dispatched (bubbling), which the page's own
// listeners see like a user's choice. Custom dropdowns are not <select>
// elements; the agent drives those with `click`.
function monocodeSelect(values) {
  const el = this;
  if (!el || el.tagName !== "SELECT") {
    return { status: "not_select" };
  }
  if (el.disabled || el.matches(":disabled")) {
    return { status: "disabled" };
  }
  const options = Array.from(el.options);
  const label = (o) => String(o.label || o.textContent || "").trim();
  const picked = [];
  const missing = [];
  for (const raw of values) {
    const value = String(raw);
    const option =
      options.find((o) => o.value === value) || options.find((o) => label(o) === value.trim());
    if (!option || option.disabled) {
      missing.push(value);
    } else if (!picked.includes(option)) {
      picked.push(option);
    }
  }
  if (missing.length > 0) {
    return {
      status: "missing",
      missing,
      available: options.slice(0, 30).map((o) => ({ value: o.value, label: label(o) })),
    };
  }
  if (!el.multiple && picked.length > 1) {
    return { status: "single" };
  }
  if (el.multiple) {
    for (const option of options) {
      option.selected = picked.includes(option);
    }
  } else if (picked.length === 1) {
    picked[0].selected = true;
  }
  el.dispatchEvent(new Event("input", { bubbles: true, composed: true }));
  el.dispatchEvent(new Event("change", { bubbles: true }));
  return {
    status: "ok",
    selected: Array.from(el.selectedOptions).map((o) => o.value),
  };
}
