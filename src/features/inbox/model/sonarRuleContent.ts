/** Convert rule HTML to our existing Markdown renderer, without injecting remote HTML. */
export function sonarHtmlToMarkdown(html: string): string {
  const document = new DOMParser().parseFromString(html, "text/html");
  const inline = (node: Node): string => {
    if (node.nodeType === 3) return node.textContent ?? "";
    if (node.nodeType !== 1) return "";
    const element = node as Element;
    const tag = element.tagName.toLowerCase();
    if (
      [
        "script",
        "style",
        "iframe",
        "object",
        "embed",
        "img",
        "svg",
        "form",
      ].includes(tag)
    )
      return "";
    const children = () => Array.from(element.childNodes).map(inline).join("");
    if (tag === "pre") {
      const code =
        element.textContent?.replace(/^\n/, "").replace(/\n$/, "") ?? "";
      const fence = "`".repeat(
        Math.max(
          3,
          ...Array.from(code.matchAll(/`+/g), (match) => match[0].length + 1),
        ),
      );
      return `\n\n${fence}\n${code}\n${fence}\n\n`;
    }
    if (tag === "code")
      return `\`${element.textContent?.replace(/`/g, "\\`") ?? ""}\``;
    if (/^h[1-6]$/.test(tag))
      return `\n\n${"#".repeat(Number(tag[1]))} ${children().trim()}\n\n`;
    if (tag === "a") {
      const label = children();
      const href = element.getAttribute("href") ?? "";
      return /^https?:\/\//i.test(href)
        ? `[${label}](${href.replace(/[\s()]/g, (value) => encodeURIComponent(value))})`
        : label;
    }
    if (tag === "strong" || tag === "b") return `**${children()}**`;
    if (tag === "em" || tag === "i") return `*${children()}*`;
    if (tag === "br") return "\n";
    if (tag === "li") return `\n- ${children().trim()}\n`;
    if (tag === "tr") return `\n${children()}\n`;
    if (tag === "td" || tag === "th") return `${children()} | `;
    if (
      ["p", "div", "section", "ul", "ol", "table", "blockquote"].includes(tag)
    )
      return `\n\n${children()}\n\n`;
    return children();
  };
  return Array.from(document.body.childNodes).map(inline).join("").trim();
}

export type SonarExampleDiff = {
  id: string;
  original: string;
  current: string;
};
export function sonarExampleDiffs(html: string): SonarExampleDiff[] {
  const document = new DOMParser().parseFromString(html, "text/html");
  const pairs = new Map<string, Partial<SonarExampleDiff>>();
  for (const block of document.querySelectorAll(
    "pre[data-diff-id][data-diff-type]",
  )) {
    const id = block.getAttribute("data-diff-id")!;
    const pair = pairs.get(id) ?? { id };
    const type = block.getAttribute("data-diff-type");
    const code = (block.textContent ?? "").replace(/^\n/, "");
    if (type === "noncompliant") pair.original = code;
    if (type === "compliant") pair.current = code;
    pairs.set(id, pair);
  }
  return [...pairs.values()].filter(
    (pair): pair is SonarExampleDiff =>
      pair.original != null && pair.current != null,
  );
}
