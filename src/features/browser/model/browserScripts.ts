/**
 * Page scripts behind the agent browser tools. Each is the body of an async
 * function run by `browser_eval`; inputs are embedded as JSON literals so no
 * agent text is ever spliced into code.
 */

const json = (value: unknown) => JSON.stringify(value ?? null);

/** Describe an element briefly, for tool results. */
const DESCRIBE = `
  function describe(el) {
    var text = (el.innerText || el.value || el.getAttribute("aria-label") || "").trim();
    if (text.length > 80) text = text.slice(0, 80) + "…";
    var tag = el.tagName.toLowerCase();
    var id = el.id ? "#" + el.id : "";
    return text ? tag + id + ' "' + text + '"' : tag + id;
  }`;

export function readScript(input: {
  format?: "text" | "html";
  selector?: string;
  maxChars?: number;
}): string {
  return `
  var selector = ${json(input.selector)};
  var format = ${json(input.format ?? "text")};
  var max = ${json(input.maxChars ?? 20000)};
  var root = selector ? document.querySelector(selector) : document.documentElement;
  if (!root) throw new Error("No element matches " + selector);
  var content = format === "html"
    ? root.outerHTML
    : (root.innerText !== undefined ? root.innerText : root.textContent) || "";
  var total = content.length;
  return {
    url: location.href,
    title: document.title,
    content: total > max ? content.slice(0, max) : content,
    truncated: total > max,
    totalChars: total,
  };`;
}

export function consoleScript(input: { limit?: number; clear?: boolean }): string {
  return `
  var entries = window.__monocodeConsole;
  if (!entries) return { entries: [], note: "Console capture starts with pages loaded after the browser tab opened." };
  var recent = entries.slice(-${json(input.limit ?? 100)});
  var total = entries.length;
  if (${json(!!input.clear)}) entries.length = 0;
  return { entries: recent, total: total };`;
}

export function clickScript(input: { selector?: string; text?: string }): string {
  return `${DESCRIBE}
  var selector = ${json(input.selector)};
  var text = ${json(input.text)};
  var el = null;
  if (selector) {
    el = document.querySelector(selector);
  } else if (text) {
    var needle = text.toLowerCase();
    var candidates = document.querySelectorAll(
      'a, button, input[type="submit"], input[type="button"], [role="button"], [role="link"], [role="menuitem"], [role="tab"], [role="option"], label, summary'
    );
    for (var i = 0; i < candidates.length; i++) {
      var c = candidates[i];
      var label = (c.innerText || c.value || c.getAttribute("aria-label") || "").toLowerCase();
      if (label.indexOf(needle) !== -1) { el = c; break; }
    }
  } else {
    throw new Error("Pass a selector or text");
  }
  if (!el) throw new Error("No element matches " + (selector || JSON.stringify(text)));
  el.scrollIntoView({ block: "center", inline: "center" });
  var rect = el.getBoundingClientRect();
  var at = { bubbles: true, cancelable: true, view: window, button: 0,
    clientX: rect.left + rect.width / 2, clientY: rect.top + rect.height / 2 };
  var described = describe(el);
  // Click after this result is stored: a navigation would otherwise discard it.
  setTimeout(function () {
    if (typeof PointerEvent === "function") el.dispatchEvent(new PointerEvent("pointerdown", at));
    el.dispatchEvent(new MouseEvent("mousedown", at));
    if (el.focus) el.focus();
    if (typeof PointerEvent === "function") el.dispatchEvent(new PointerEvent("pointerup", at));
    el.dispatchEvent(new MouseEvent("mouseup", at));
    el.click();
  }, 0);
  return { clicked: described };`;
}

export function typeScript(input: {
  selector: string;
  text: string;
  clear?: boolean;
  submit?: boolean;
}): string {
  return `${DESCRIBE}
  var selector = ${json(input.selector)};
  var text = ${json(input.text)};
  var clear = ${json(input.clear !== false)};
  var submit = ${json(!!input.submit)};
  var el = document.querySelector(selector);
  if (!el) throw new Error("No element matches " + selector);
  el.scrollIntoView({ block: "center" });
  if (el.focus) el.focus();
  if (el.isContentEditable) {
    if (clear) el.textContent = "";
    document.execCommand("insertText", false, text);
  } else if (el instanceof HTMLSelectElement) {
    var option = Array.prototype.find.call(el.options, function (o) {
      return o.value === text || o.text.trim() === text;
    });
    if (!option) throw new Error("No option " + JSON.stringify(text) + " in " + selector);
    el.value = option.value;
  } else if (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement) {
    // Use the prototype setter so frameworks that track value (React) notice.
    var proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    var setter = Object.getOwnPropertyDescriptor(proto, "value").set;
    setter.call(el, (clear ? "" : el.value) + text);
    el.dispatchEvent(new InputEvent("input", { bubbles: true, data: text, inputType: "insertText" }));
  } else {
    throw new Error(describe(el) + " is not editable");
  }
  el.dispatchEvent(new Event("change", { bubbles: true }));
  var value = "value" in el ? el.value : el.textContent;
  if (submit) {
    setTimeout(function () {
      var form = el.form || el.closest("form");
      if (form && form.requestSubmit) form.requestSubmit();
      else if (form) form.submit();
      else {
        var key = { key: "Enter", code: "Enter", keyCode: 13, bubbles: true, cancelable: true };
        el.dispatchEvent(new KeyboardEvent("keydown", key));
        el.dispatchEvent(new KeyboardEvent("keyup", key));
      }
    }, 0);
  }
  return { typed: describe(el), value: value, submitted: submit };`;
}

export function waitScript(input: {
  selector?: string;
  text?: string;
  timeoutMs: number;
}): string {
  return `
  var selector = ${json(input.selector)};
  var text = ${json(input.text)};
  if (!selector && !text) throw new Error("Pass a selector or text");
  var deadline = Date.now() + ${json(input.timeoutMs)};
  function found() {
    if (selector) return !!document.querySelector(selector);
    return !!document.body && document.body.innerText.indexOf(text) !== -1;
  }
  while (!found()) {
    if (Date.now() > deadline) {
      throw new Error("Timed out waiting for " + (selector || JSON.stringify(text)));
    }
    await new Promise(function (resolve) { setTimeout(resolve, 100); });
  }
  return { found: true, url: location.href, title: document.title };`;
}
