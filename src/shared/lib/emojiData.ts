import type { EmojiData, SkinTone } from "frimousse";

/** The emoji fonts each platform ships, tried in order. */
export const EMOJI_FONT =
  "'Apple Color Emoji', 'Noto Color Emoji', 'Twemoji Mozilla', 'Android Emoji', 'Segoe UI Emoji', 'Segoe UI Symbol', EmojiSymbols, sans-serif";

const TONES: Exclude<SkinTone, "none">[] = [
  "light",
  "medium-light",
  "medium",
  "medium-dark",
  "dark",
];

let loading: Promise<EmojiData> | undefined;

/**
 * The picker's emoji, from the copy of Emojibase bundled with the app, so it
 * works offline and never calls a CDN. The data loads on first open, and
 * only emoji this system's font can draw are kept: a newer one would show
 * as an empty box.
 */
export function loadEmojiData(): Promise<EmojiData> {
  loading ??= buildEmojiData().catch((error: unknown) => {
    loading = undefined;
    throw error;
  });
  return loading;
}

async function buildEmojiData(): Promise<EmojiData> {
  const [{ default: emojis }, { default: messages }] = await Promise.all([
    import("emojibase-data/en/data.json"),
    import("emojibase-data/en/messages.json"),
  ]);
  const countryFlags = messages.subgroups.find(
    (subgroup) => subgroup.key === "country-flag",
  )?.order;
  // Skin-tone pieces are not emoji on their own.
  const grouped = emojis.filter(
    (emoji) => emoji.group !== undefined && emoji.group !== 2,
  );
  const version = drawableVersion(grouped);
  const flags = canDraw("🇪🇺");
  return {
    locale: "en",
    categories: messages.groups
      .filter((group) => group.key !== "component")
      .map((group) => ({
        index: group.order,
        label: capitalize(group.message),
      })),
    skinTones: Object.fromEntries(
      messages.skinTones.map((tone) => [tone.key, capitalize(tone.message)]),
    ) as EmojiData["skinTones"],
    emojis: grouped.flatMap((emoji) => {
      const flag = emoji.subgroup === countryFlags;
      if (emoji.version > version || (flag && !flags)) return [];
      // One tone across the whole emoji is a skin; mixed tones (two people)
      // are other spellings of it.
      const toned = emoji.skins?.filter(
        (skin) => typeof skin.tone === "number",
      );
      const mixed = emoji.skins
        ?.filter((skin) => Array.isArray(skin.tone))
        .map((skin) => skin.emoji);
      return [
        {
          emoji: emoji.emoji,
          category: emoji.group!,
          version: emoji.version,
          label: capitalize(emoji.label),
          tags: emoji.tags ?? [],
          countryFlag: flag || undefined,
          skins: toned?.length
            ? (Object.fromEntries(
                toned.map((skin) => [
                  TONES[(skin.tone as number) - 1],
                  skin.emoji,
                ]),
              ) as Record<Exclude<SkinTone, "none">, string>)
            : undefined,
          aliases: mixed?.length ? mixed : undefined,
        },
      ];
    }),
  };
}

/** The newest Emoji version this system draws, judged by one of its emoji. */
function drawableVersion(
  emojis: readonly { emoji: string; version: number }[],
): number {
  const samples = new Map<number, string>();
  for (const emoji of emojis)
    if (!samples.has(emoji.version)) samples.set(emoji.version, emoji.emoji);
  const versions = [...samples.keys()].sort((a, b) => b - a);
  return (
    versions.find((version) => canDraw(samples.get(version)!)) ??
    versions[versions.length - 1] ??
    0
  );
}

/**
 * Whether the emoji renders as one color glyph. Drawn in two text colors, a
 * real emoji keeps its own colors; a fallback takes the text color, or
 * falls apart into several glyphs that measure wider.
 */
function canDraw(emoji: string): boolean {
  let context: CanvasRenderingContext2D | null = null;
  try {
    context = document
      .createElement("canvas")
      .getContext("2d", { willReadFrequently: true });
  } catch {
    // No canvas to ask, so trust the font.
  }
  if (!context) return true;
  const size = 2;
  context.canvas.width = size;
  context.canvas.height = size;
  context.font = `${size}px ${EMOJI_FONT}`;
  context.textBaseline = "middle";
  if (context.measureText(emoji).width >= size * 2) return false;
  context.fillStyle = "#00f";
  context.fillText(emoji, 0, 0);
  const blue = context.getImageData(0, 0, size, size).data;
  context.clearRect(0, 0, size, size);
  context.fillStyle = "#f00";
  context.fillText(emoji, 0, 0);
  const red = context.getImageData(0, 0, size, size).data;
  return blue.every((value, index) => value === red[index]);
}

function capitalize(text: string): string {
  return text.charAt(0).toUpperCase() + text.slice(1);
}

const RECENT_KEY = "monocode.recentEmoji";
const SKIN_TONE_KEY = "monocode.emojiSkinTone";
const RECENT_MAX = 16;

/** Emoji picked lately, newest first. */
export function loadRecentEmoji(): string[] {
  try {
    const value: unknown = JSON.parse(localStorage.getItem(RECENT_KEY) ?? "[]");
    return Array.isArray(value)
      ? value
          .filter((item): item is string => typeof item === "string")
          .slice(0, RECENT_MAX)
      : [];
  } catch {
    return [];
  }
}

/** Moves a pick to the front of the recents and returns the new list. */
export function rememberEmoji(emoji: string): string[] {
  const next = [
    emoji,
    ...loadRecentEmoji().filter((item) => item !== emoji),
  ].slice(0, RECENT_MAX);
  try {
    localStorage.setItem(RECENT_KEY, JSON.stringify(next));
  } catch {
    // Recents are a convenience; a full or blocked store just forgets them.
  }
  return next;
}

export function loadEmojiSkinTone(): SkinTone {
  try {
    const value = localStorage.getItem(SKIN_TONE_KEY);
    return value && (TONES as string[]).includes(value)
      ? (value as SkinTone)
      : "none";
  } catch {
    return "none";
  }
}

export function saveEmojiSkinTone(tone: SkinTone): void {
  try {
    if (tone === "none") localStorage.removeItem(SKIN_TONE_KEY);
    else localStorage.setItem(SKIN_TONE_KEY, tone);
  } catch {
    // Falls back to the default tone next time.
  }
}
