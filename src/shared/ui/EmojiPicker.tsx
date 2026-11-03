import { useEffect, useRef, useState } from "react";
import {
  EmojiPicker as Picker,
  type EmojiPickerListComponents,
  type SkinTone,
} from "frimousse";
import { Search } from "./icons";
import { Popover, type PopoverAnchor } from "./Popover";
import {
  EMOJI_FONT,
  loadEmojiData,
  loadEmojiSkinTone,
  loadRecentEmoji,
  rememberEmoji,
  saveEmojiSkinTone,
} from "../lib/emojiData";

const COLUMNS = 8;

/** One face per Emojibase group, for the bar that jumps to it. */
const CATEGORY_ICONS = ["😀", "👋", "🐻", "🍔", "🚗", "⚽", "💡", "🔣", "🏳️"];

const CELL =
  "grid size-8 place-items-center rounded-md text-[20px] leading-none outline-none";

// Module-level, so the list's memoized rows keep their components.
const LIST_COMPONENTS: EmojiPickerListComponents = {
  CategoryHeader: ({ category, ...props }) => (
    <div
      {...props}
      className="px-1 pt-2 pb-1 text-[11px] font-medium text-content/45"
    >
      {category.label}
    </div>
  ),
  Row: ({ children, ...props }) => (
    <div {...props} className="flex">
      {children}
    </div>
  ),
  Emoji: ({ emoji, ...props }) => (
    <button
      {...props}
      title={emoji.label}
      className={`${CELL} ${emoji.isActive ? "bg-content/10" : ""}`}
    >
      {emoji.emoji}
    </button>
  ),
};

/**
 * An emoji picker that looks and works the same on every platform: search,
 * recents, categories and a skin tone, with the data bundled in the app.
 * Picking hands the emoji over and closes; Escape or a click outside closes.
 */
export function EmojiPickerPopover({
  anchor,
  onPick,
  onDismiss,
}: {
  anchor: PopoverAnchor;
  onPick: (emoji: string) => void;
  onDismiss: () => void;
}) {
  const [search, setSearch] = useState("");
  const [recent] = useState(loadRecentEmoji);
  const [initialTone] = useState(loadEmojiSkinTone);
  const viewport = useRef<HTMLDivElement>(null);
  const searchField = useRef<HTMLInputElement>(null);
  // The popover measures itself hidden first, which `autoFocus` cannot
  // reach, so focus search once it is placed.
  useEffect(() => {
    const frame = requestAnimationFrame(() => searchField.current?.focus());
    return () => cancelAnimationFrame(frame);
  }, []);
  const pick = (emoji: string) => {
    rememberEmoji(emoji);
    onPick(emoji);
  };
  const jumpTo = (index: number) => {
    const el = viewport.current;
    // The list keeps a hidden copy of a category to measure; skip it.
    const category = [
      ...(el?.querySelectorAll<HTMLElement>("[frimousse-category]") ?? []),
    ].filter((entry) => !entry.closest("[aria-hidden]"))[index];
    if (el && category) el.scrollTo({ top: category.offsetTop });
  };

  return (
    <Popover
      anchor={anchor}
      side="top"
      align="start"
      gap={6}
      width={COLUMNS * 32 + 26}
      onDismiss={onDismiss}
    >
      <Picker.Root
        data-emoji-picker=""
        resolveEmojiData={loadEmojiData}
        columns={COLUMNS}
        skinTone={initialTone}
        sticky={false}
        onEmojiSelect={(emoji) => pick(emoji.emoji)}
        className="flex h-96 flex-col font-sans"
      >
        <div className="p-2 pb-1">
          <label className="flex h-8 items-center gap-2 rounded-md border border-content/10 bg-content/5 px-2 focus-within:border-accent/60">
            <Search className="size-3.5 shrink-0 text-content/45" />
            <Picker.Search
              ref={searchField}
              aria-label="Search emoji"
              placeholder="Search emoji"
              value={search}
              onChange={(event) => setSearch(event.target.value)}
              className="min-w-0 flex-1 bg-transparent text-[13px] text-content outline-none placeholder:text-content/35 [&::-webkit-search-cancel-button]:hidden"
            />
          </label>
        </div>
        {recent.length > 0 && !search ? (
          <div className="px-2">
            <p className="px-1 pt-1 pb-1 text-[11px] font-medium text-content/45">
              Recent
            </p>
            <div className="flex flex-wrap">
              {recent.slice(0, COLUMNS).map((emoji) => (
                <button
                  key={emoji}
                  type="button"
                  aria-label={emoji}
                  onClick={() => pick(emoji)}
                  style={{ fontFamily: EMOJI_FONT }}
                  className={`${CELL} hover:bg-content/10 focus-visible:bg-content/10`}
                >
                  {emoji}
                </button>
              ))}
            </div>
          </div>
        ) : null}
        <Picker.Viewport
          ref={viewport}
          className="relative min-h-0 flex-1 px-2 outline-none"
        >
          <Picker.Loading className="absolute inset-0 grid place-items-center text-[12px] text-content/45">
            Loading…
          </Picker.Loading>
          <Picker.Empty className="absolute inset-0 grid place-items-center text-[12px] text-content/45">
            No emoji found
          </Picker.Empty>
          <Picker.List
            className="select-none pb-1"
            components={LIST_COMPONENTS}
          />
        </Picker.Viewport>
        <div className="flex h-10 shrink-0 items-center gap-1 border-t border-content/10 px-2">
          {search ? (
            <Picker.ActiveEmoji>
              {({ emoji }) => (
                <p className="min-w-0 flex-1 truncate px-1 text-[12px] text-content/55">
                  {emoji ? emoji.label : "Pick an emoji"}
                </p>
              )}
            </Picker.ActiveEmoji>
          ) : (
            <div className="flex min-w-0 flex-1">
              {CATEGORY_ICONS.map((icon, index) => (
                <button
                  key={icon}
                  type="button"
                  aria-label={`Jump to category ${index + 1}`}
                  onClick={() => jumpTo(index)}
                  style={{ fontFamily: EMOJI_FONT }}
                  className="grid h-7 min-w-0 flex-1 place-items-center rounded-md text-[15px] leading-none opacity-70 grayscale hover:bg-content/10 hover:opacity-100 hover:grayscale-0"
                >
                  {icon}
                </button>
              ))}
            </div>
          )}
          <Picker.SkinTone>
            {({ skinTone, setSkinTone, skinToneVariations }) => {
              const index = skinToneVariations.findIndex(
                (variation) => variation.skinTone === skinTone,
              );
              const next: SkinTone =
                skinToneVariations[(index + 1) % skinToneVariations.length]
                  .skinTone;
              return (
                <button
                  type="button"
                  title="Skin tone"
                  aria-label="Change skin tone"
                  onClick={() => {
                    setSkinTone(next);
                    saveEmojiSkinTone(next);
                  }}
                  style={{ fontFamily: EMOJI_FONT }}
                  className="grid size-7 shrink-0 place-items-center rounded-md text-[15px] leading-none hover:bg-content/10"
                >
                  {skinToneVariations[Math.max(0, index)].emoji}
                </button>
              );
            }}
          </Picker.SkinTone>
        </div>
      </Picker.Root>
    </Popover>
  );
}
