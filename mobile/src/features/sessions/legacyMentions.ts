import { mentionTokenAt } from "./mentionToken";

export type LiteralMention = { label: string; path: string; isDir: boolean };

/** Released desktops have no alias metadata for spaced paths. Spell out only
 * those selected references as literal paths before the normal desktop send
 * pipeline. This avoids guessing a desktop alias that could name another file. */
export function prepareLiteralMentions(
  text: string,
  references: LiteralMention[],
): string {
  const refs = [...references].sort((a, b) => b.label.length - a.label.length);
  return text.replace(
    /(^|\s)@(\S+)/g,
    (match, prefix: string, raw: string, offset: number) => {
      if (!mentionTokenAt(text, offset + prefix.length + 1)) return match;
      const ref = refs.find(
        ({ label }) =>
          raw === label ||
          (raw.startsWith(label) &&
            /^[,;:!?)}\]"'.\/]+$/.test(raw.slice(label.length))),
      );
      if (!ref) return match;
      const path = ref.path + (ref.isDir ? "/" : "");
      const literal = path.includes("`") ? JSON.stringify(path) : `\`${path}\``;
      return `${prefix}${literal}${raw.slice(ref.label.length)}`;
    },
  );
}
