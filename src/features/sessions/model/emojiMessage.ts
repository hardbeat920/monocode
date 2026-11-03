/** Messaging apps show up to three emoji on their own, without a bubble. */
const MAX_EMOJI = 3;
const PICTURE = String.raw`\p{Extended_Pictographic}(?:️|\p{Emoji_Modifier})?`;
/** One emoji: a flag, a keycap, or pictures joined into one (a family). */
const EMOJI = String.raw`\p{Regional_Indicator}{2}|[#*0-9]️?⃣|${PICTURE}(?:‍${PICTURE})*`;
const EMOJI_ONLY = new RegExp(
  String.raw`^\s*(?:(?:${EMOJI})\s*){1,${MAX_EMOJI}}$`,
  "u",
);
const ONE_EMOJI = new RegExp(String.raw`^\s*(?:${EMOJI})\s*$`, "u");

/** True when a message is just a few emoji, which read as a reaction. */
export function isEmojiOnlyMessage(text: string): boolean {
  return EMOJI_ONLY.test(text);
}

/** True when the text is exactly one emoji, as a reaction is. */
export function isSingleEmoji(text: string): boolean {
  return ONE_EMOJI.test(text);
}
