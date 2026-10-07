import { describe, expect, it } from "vitest";
import { isEmojiOnlyMessage } from "./emojiMessage";

describe("isEmojiOnlyMessage", () => {
  it.each(["🔥", " 🔥 ", "👍🏽", "❤️", "👨‍👩‍👧", "🇬🇧", "1️⃣", "🔥 🔥🔥"])(
    "takes %j as a reaction",
    (text) => expect(isEmojiOnlyMessage(text)).toBe(true),
  );

  it.each(["", "   ", "🔥 nice", "ok", "123", "#", "🔥🔥🔥🔥", ":)"])(
    "keeps %j in a bubble",
    (text) => expect(isEmojiOnlyMessage(text)).toBe(false),
  );
});
