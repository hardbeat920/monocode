import remend from "remend";
import { parseMarkdownIntoBlocks } from "streamdown";

export function isFenceBlock(content: string): boolean {
  return /^ {0,3}(?:`{3,}|~{3,})/.test(content);
}

/**
 * Repair incomplete prose after splitting it into blocks. Remend scans back
 * through the source for each emphasis/link marker; running it over a large
 * Markdown fence can become quadratic, even though its contents are literal.
 * The Markdown parser already accepts open fences, so leave those untouched.
 */
export function parseStreamingMarkdown(text: string): string[] {
  return parseMarkdownIntoBlocks(text).map((block) =>
    isFenceBlock(block) ? block : remend(block),
  );
}
