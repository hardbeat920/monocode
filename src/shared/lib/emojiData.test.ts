// @vitest-environment happy-dom
import { beforeEach, expect, it, vi } from "vitest";
import {
  loadEmojiData,
  loadEmojiSkinTone,
  loadRecentEmoji,
  rememberEmoji,
  saveEmojiSkinTone,
} from "./emojiData";

const stored = new Map<string, string>();
beforeEach(() => {
  stored.clear();
  vi.stubGlobal("localStorage", {
    getItem: (key: string) => stored.get(key) ?? null,
    setItem: (key: string, value: string) => stored.set(key, value),
    removeItem: (key: string) => stored.delete(key),
  });
});

it("builds picker data from the bundled Emojibase copy", async () => {
  const data = await loadEmojiData();
  expect(data.categories.map((category) => category.label)).toEqual([
    "Smileys & emotion",
    "People & body",
    "Animals & nature",
    "Food & drink",
    "Travel & places",
    "Activities",
    "Objects",
    "Symbols",
    "Flags",
  ]);
  const grinning = data.emojis.find((emoji) => emoji.emoji === "😀");
  expect(grinning).toMatchObject({ label: "Grinning face", category: 0 });
  // One tone per skin; skin-tone swatches are not offered on their own.
  expect(data.emojis.find((emoji) => emoji.emoji === "👋")?.skins?.dark).toBe(
    "👋🏿",
  );
  expect(data.emojis.some((emoji) => emoji.category === 2)).toBe(false);
  expect(data.skinTones.light).toBe("Light skin tone");
  // Loaded once and shared.
  expect(await loadEmojiData()).toBe(data);
});

it("keeps recent picks newest first without repeats", () => {
  expect(loadRecentEmoji()).toEqual([]);
  rememberEmoji("😀");
  rememberEmoji("🎉");
  expect(rememberEmoji("😀")).toEqual(["😀", "🎉"]);
  expect(loadRecentEmoji()).toEqual(["😀", "🎉"]);
  for (let index = 0; index < 20; index++)
    rememberEmoji(String.fromCodePoint(0x1f600 + index));
  expect(loadRecentEmoji()).toHaveLength(16);
});

it("remembers a skin tone and ignores unknown ones", () => {
  expect(loadEmojiSkinTone()).toBe("none");
  saveEmojiSkinTone("medium-dark");
  expect(loadEmojiSkinTone()).toBe("medium-dark");
  stored.set("monocode.emojiSkinTone", "purple");
  expect(loadEmojiSkinTone()).toBe("none");
  saveEmojiSkinTone("none");
  expect(stored.has("monocode.emojiSkinTone")).toBe(false);
});
