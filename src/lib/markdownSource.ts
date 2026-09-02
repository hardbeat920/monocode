/** ATX heading at the start of a source line (`#` through `######`). */
export function isAtxHeadingLine(line: string): boolean {
  return /^\s{0,3}#{1,6}(?:\s|$)/.test(line);
}

/**
 * Rewrite `file://` link targets to direct filesystem paths so Markdown
 * sanitizers (e.g. `rehype-harden`) preserve them and `MarkdownLink` can open
 * them in the editor.
 */
export function normalizeFileLinks(text: string): string {
  if (!text || !text.includes("file:")) return text;
  const cleanPath = (raw: string) => {
    let path = raw.replace(/^file:\/\/(?:localhost)?\/?/i, "");
    if (/^[A-Za-z]:[/\\]/.test(path)) {
      path = path.replace(/\\/g, "/");
    } else {
      path = `/${path.replace(/\\/g, "/")}`;
    }
    try {
      path = decodeURI(path);
    } catch {}
    return path;
  };

  return text
    .replace(/\]\(<(file:\/\/[^>]+)>\)/g, (_, url) => {
      const hashMatch = url.match(/(#[^>]*)$/);
      const hash = hashMatch ? hashMatch[1] : "";
      const withoutHash = url.slice(0, url.length - hash.length);
      return `](${cleanPath(withoutHash)}${hash})`;
    })
    .replace(/\]\((file:\/\/[^)]+)\)/g, (_, url) => {
      const hashMatch = url.match(/(#[^)]*)$/);
      const hash = hashMatch ? hashMatch[1] : "";
      const withoutHash = url.slice(0, url.length - hash.length);
      return `](${cleanPath(withoutHash)}${hash})`;
    })
    .replace(/<(file:\/\/[^>]+)>/g, (_, url) => {
      const hashMatch = url.match(/(#[^>]*)$/);
      const hash = hashMatch ? hashMatch[1] : "";
      const withoutHash = url.slice(0, url.length - hash.length);
      return `<${cleanPath(withoutHash)}${hash}>`;
    });
}

