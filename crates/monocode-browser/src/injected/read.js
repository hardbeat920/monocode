// `read`: the text of an element or of the whole document,
// as plain text (innerText) or as simple Markdown, sliced to
// [offset, offset + limit) so long pages are read in several calls.
//
// `this` is the node when a ref was given (an element, or a text node from
// a full snapshot, whose element is read), otherwise the isolated world's
// global object and the document body is read. Read-only.
function monocodeRead(format, offset, limit) {
  const node = this && typeof this.nodeType === "number" ? this : null;
  const root = node
    ? node.nodeType === 1
      ? node
      : node.parentElement
    : document.body || document.documentElement || null;
  if (!root) {
    return { text: "", total: 0 };
  }
  const visible = (e) =>
    typeof e.checkVisibility !== "function" || e.checkVisibility({ visibilityProperty: true });
  const skip = new Set(["script", "style", "noscript", "template", "svg", "canvas", "head"]);

  const markdown = (start) => {
    const out = [];
    const inline = (node) => {
      if (node.nodeType === 3) {
        return node.nodeValue.replace(/\s+/g, " ");
      }
      if (node.nodeType !== 1) {
        return "";
      }
      const tag = node.tagName.toLowerCase();
      if (skip.has(tag) || !visible(node)) {
        return "";
      }
      const inner = () => Array.from(node.childNodes).map(inline).join("");
      switch (tag) {
        case "br":
          return "\n";
        case "a": {
          const text = inner().trim();
          const href = node.href || "";
          return href && text ? "[" + text + "](" + href + ")" : text;
        }
        case "strong":
        case "b":
          return "**" + inner().trim() + "**";
        case "em":
        case "i":
          return "*" + inner().trim() + "*";
        case "code":
          return "`" + inner() + "`";
        case "img":
          return node.alt ? "![" + node.alt + "]" : "";
        default:
          return inner();
      }
    };
    const block = (node, prefix) => {
      if (node.nodeType === 3) {
        const text = node.nodeValue.replace(/\s+/g, " ").trim();
        if (text) {
          out.push(prefix + text);
        }
        return;
      }
      if (node.nodeType !== 1) {
        return;
      }
      const tag = node.tagName.toLowerCase();
      if (skip.has(tag) || !visible(node)) {
        return;
      }
      const heading = /^h([1-6])$/.exec(tag);
      if (heading) {
        out.push("", "#".repeat(Number(heading[1])) + " " + inline(node).trim(), "");
        return;
      }
      switch (tag) {
        case "p":
          out.push("", prefix + inline(node).trim(), "");
          return;
        case "pre":
          out.push("", "```", node.innerText || node.textContent || "", "```", "");
          return;
        case "blockquote":
          for (const child of node.childNodes) {
            block(child, prefix + "> ");
          }
          return;
        case "ul":
        case "ol": {
          let n = 1;
          for (const child of node.children) {
            if (child.tagName.toLowerCase() !== "li") {
              continue;
            }
            const marker = tag === "ol" ? n++ + ". " : "- ";
            out.push(prefix + marker + inline(child).trim());
          }
          return;
        }
        case "tr": {
          const cells = Array.from(node.children).map((c) => inline(c).trim().replace(/\|/g, "\\|"));
          out.push(prefix + "| " + cells.join(" | ") + " |");
          return;
        }
        case "a":
        case "span":
        case "strong":
        case "b":
        case "em":
        case "i":
        case "code":
        case "label":
        case "button": {
          const text = inline(node).trim();
          if (text) {
            out.push(prefix + text);
          }
          return;
        }
        default:
          for (const child of node.childNodes) {
            block(child, prefix);
          }
      }
    };
    block(start, "");
    return out
      .join("\n")
      .replace(/\n{3,}/g, "\n\n")
      .trim();
  };

  const full =
    format === "markdown"
      ? markdown(root)
      : String(root.innerText !== undefined ? root.innerText : root.textContent || "");
  const from = Math.max(0, Math.min(Number(offset) || 0, full.length));
  return { text: full.slice(from, from + limit), total: full.length };
}
