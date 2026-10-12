import { isSingleEmoji } from "../../sessions/model/emojiMessage";
import type { Block } from "../../sessions/model/session";

/**
 * A Mono can answer with only an emoji on the user's message, as a friend
 * would to "cool, thanks". It says so by making its whole reply this tag; the
 * chat hides that reply and pins the emoji to the message it answers.
 */
const OPEN = "<reaction>";
const REACTION = /^\s*<reaction>\s*([^<]{1,32}?)\s*<\/reaction>\s*$/u;

/** The emoji when a reply is only a reaction. */
export function monoReaction(text: string): string | undefined {
  const emoji = REACTION.exec(text)?.[1];
  return emoji && isSingleEmoji(emoji) ? emoji : undefined;
}

/** A reply still streaming in that may yet turn out to be a reaction. */
function partialReaction(block: Block): boolean {
  if (!block.streaming) return false;
  const text = block.text.trimStart();
  return text.length < OPEN.length
    ? OPEN.startsWith(text)
    : text.startsWith(OPEN) && !text.includes("</");
}

/** How a reply reads outside the chat, such as in a notification. */
export function monoReplyText(text: string): string {
  const emoji = monoReaction(text);
  return emoji ? `Reacted ${emoji} to your message` : text;
}

const reacted = new WeakMap<Block, Map<string, Block>>();
/** The same copy each time, so unchanged turns keep their identity. */
function withReaction(block: Block, emoji: string, key: keyof Block): Block {
  let byEmoji = reacted.get(block);
  if (!byEmoji) reacted.set(block, (byEmoji = new Map()));
  const id = `${key}:${emoji}`;
  let copy = byEmoji.get(id);
  if (!copy) {
    copy = { ...block, [key]: emoji };
    byEmoji.set(id, copy);
  }
  return copy;
}

const results = new WeakMap<Block[], Block[]>();
/**
 * The chat's blocks with each reaction moved onto the user's message before
 * it. One with no message to sit on, like a reply to a session's report,
 * shows as the emoji on its own.
 */
export function monoReactionBlocks(blocks: Block[]): Block[] {
  const cached = results.get(blocks);
  if (cached) return cached;
  const out: Block[] = [];
  let target = -1;
  let changed = false;
  for (const block of blocks) {
    if (block.role === "user") {
      if (!block.internal) target = out.length;
      out.push(block);
      continue;
    }
    if (block.role !== "assistant" || block.tool) {
      out.push(block);
      continue;
    }
    if (partialReaction(block)) {
      changed = true;
      continue;
    }
    const emoji = monoReaction(block.text);
    if (!emoji) {
      out.push(block);
      continue;
    }
    changed = true;
    if (target >= 0) {
      // The reaction is the whole answer: the thinking and steps behind it
      // would only draw an empty reply. Errors still show.
      const after = out.splice(target + 1);
      out.push(...after.filter((step) => step.role === "system" && step.notice));
      out[target] = withReaction(out[target], emoji, "monoReaction");
    } else out.push(withReaction(block, emoji, "text"));
  }
  const result = changed ? out : blocks;
  results.set(blocks, result);
  return result;
}
