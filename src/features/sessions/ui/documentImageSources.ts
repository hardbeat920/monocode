import type { Element, Root } from "hast";

/*
 * A Markdown document's images are paths in its project, which MarkdownImage
 * resolves and keeps inside the project itself. rehype-harden would rebase
 * them as URLs first, turning `../a.png` into `/a.png` and dropping a bare
 * `a.png`, so a document holds each image's path aside while harden runs and
 * puts it back afterwards. Links are still hardened as usual.
 */

const heldSources = new WeakMap<Element, string>();

/** A source harden keeps, so the image is still there to take its path back. */
const PLACEHOLDER_SRC = "/";

function eachImage(node: Root | Element, visit: (image: Element) => void) {
  for (const child of node.children) {
    if (child.type !== "element") continue;
    if (child.tagName === "img") visit(child);
    eachImage(child, visit);
  }
}

/** Runs just before harden. */
export function rehypeHoldImageSources() {
  return (tree: Root) =>
    eachImage(tree, (image) => {
      const src = image.properties.src;
      heldSources.set(image, typeof src === "string" ? src : "");
      image.properties.src = PLACEHOLDER_SRC;
    });
}

/** Runs just after harden. */
export function rehypeRestoreImageSources() {
  return (tree: Root) =>
    eachImage(tree, (image) => {
      const src = heldSources.get(image);
      if (src === undefined) return;
      heldSources.delete(image);
      image.properties.src = src;
    });
}
