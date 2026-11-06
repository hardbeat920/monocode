import type { InboxClient } from "./protocol";

/**
 * The address of each image in a body, in the host's order: every `![alt](url)`
 * and `<img src="url">`, code and comments included. `inbox.media` counts these
 * to find an image, so this reads the text as `image_urls` does in
 * `monocode-core/src/mobile/inbox.rs`, not as the page shows it: an example in
 * a code fence still takes up a number.
 */
export function imageUrls(markdown: string): string[] {
  const found: { at: number; url: string }[] = [];
  let at = 0;
  for (;;) {
    const open = markdown.indexOf("![", at);
    if (open < 0) break;
    at = open + 2;
    const close = markdown.indexOf("](", at);
    if (close < 0) break;
    const start = close + 2;
    const end = markdown.indexOf(")", start);
    if (end < 0) break;
    const [first = ""] = markdown.slice(start, end).trim().split(/\s+/);
    const url = first.replace(/^[<>]+|[<>]+$/g, "");
    if (url) found.push({ at: open, url });
    at = end;
  }
  // ASCII only, so offsets here stay offsets in the text.
  const lower = markdown.replace(/[A-Z]+/g, (letters) => letters.toLowerCase());
  at = 0;
  for (;;) {
    const open = lower.indexOf("<img", at);
    if (open < 0) break;
    const close = lower.indexOf(">", open);
    const end = close < 0 ? lower.length : close;
    const src = lower.slice(open, end).indexOf("src=");
    if (src >= 0) {
      const rest = markdown.slice(open + src + 4, end);
      const quote = rest[0];
      const stop = quote === '"' || quote === "'" ? rest.indexOf(quote, 1) : -1;
      if (stop > 0) found.push({ at: open, url: rest.slice(1, stop) });
    }
    at = Math.max(end, open + 4);
  }
  return found.sort((a, b) => a.at - b.at).map((image) => image.url);
}

/** Where an image is read from: the item's body, or one comment in its thread. */
export type ImagePlace = { itemKey: string; commentId?: string };

/** Each image costs the desktop a read from the provider, so a few go at once. */
const MAX_READS = 3;
/** Pictures kept for sheets opened again; each can be megabytes of text. */
const MAX_KEPT = 12;

let reads = 0;
const waiting: (() => void)[] = [];
const kept = new Map<string, string>();
const reading = new Map<string, Promise<string>>();

async function takingTurns<T>(read: () => Promise<T>): Promise<T> {
  if (reads >= MAX_READS) await new Promise<void>((resolve) => waiting.push(resolve));
  else reads += 1;
  try {
    return await read();
  } finally {
    // A waiter takes this read's place rather than counting itself in.
    const next = waiting.shift();
    if (next) next();
    else reads -= 1;
  }
}

function keyOf({ itemKey, commentId }: ImagePlace, src: string) {
  return `${itemKey}\n${commentId ?? ""}\n${src}`;
}

/** The picture already read for this image, to show at once. */
export function keptImage(place: ImagePlace, src: string): string | undefined {
  return kept.get(keyOf(place, src));
}

/**
 * An image as a data URI, read through the desktop: it holds the provider's
 * token and the list of hosts it may fetch from, and the phone never fetches
 * an address itself. A failed read is not kept, so opening the sheet tries again.
 */
export function readImage(
  client: Pick<InboxClient, "media">,
  place: ImagePlace,
  { src, index }: { src: string; index: number },
): Promise<string> {
  const key = keyOf(place, src);
  const known = kept.get(key);
  if (known) return Promise.resolve(known);
  const pending = reading.get(key);
  if (pending) return pending;
  const read = takingTurns(() =>
    client.media(
      place.itemKey,
      place.commentId === undefined
        ? { index }
        : { commentId: place.commentId, commentIndex: index },
    ),
  )
    .then((media) => {
      const uri = `data:${media.mime};base64,${media.base64}`;
      kept.set(key, uri);
      const oldest = kept.keys().next().value;
      if (kept.size > MAX_KEPT && oldest !== undefined) kept.delete(oldest);
      return uri;
    })
    .finally(() => reading.delete(key));
  reading.set(key, read);
  return read;
}
