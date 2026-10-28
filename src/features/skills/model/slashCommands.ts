import { fuzzyMatch } from "../../../shared/lib/fuzzy";
import { isMarkdownBlockquotePosition } from "../../sessions/model/quoteDraft";
import type { Skill } from "./skills";

// Keep picker helpers independent of skill discovery for the floating composer.
export type SlashToken = {
  start: number;
  end: number;
  query: string;
};

const MAX_PICKER = 50;

export function rankSkills(
  skills: Skill[],
  query: string,
  limit = MAX_PICKER,
): Skill[] {
  const needle = query.trim().toLowerCase();
  if (!needle) {
    return [...skills]
      .sort((a, b) => {
        const rank = scopeRank(a) - scopeRank(b);
        if (rank !== 0) return rank;
        return a.name.localeCompare(b.name);
      })
      .slice(0, limit);
  }

  const scored: { skill: Skill; score: number }[] = [];
  for (const skill of skills) {
    const nameHit = fuzzyMatch(needle, skill.name);
    const invocationHit = nameHit
      ? null
      : fuzzyMatch(
          needle,
          [
            skill.invocation,
            ...(skill.kind === "native" ? (skill.aliases ?? []) : []),
          ].join(" "),
        );
    const descHit =
      nameHit || invocationHit ? null : fuzzyMatch(needle, skill.description);
    const hit = nameHit ?? invocationHit ?? descHit;
    if (!hit) continue;
    const score = nameHit || invocationHit ? hit.score + 400 : hit.score;
    scored.push({ skill, score });
  }
  scored.sort((a, b) => {
    if (b.score !== a.score) return b.score - a.score;
    return a.skill.name.localeCompare(b.skill.name);
  });
  return scored.slice(0, limit).map((row) => row.skill);
}

function scopeRank(skill: Skill): number {
  if (skill.kind === "builtin") return 0;
  if (skill.kind === "native" || skill.scope === "project") return 1;
  return 2;
}

/** Slash token that contains `cursor`, if the user is typing `/skill`. */
export function slashTokenAt(
  text: string,
  cursor: number,
  native = false,
): SlashToken | null {
  const i = clamp(cursor, 0, text.length);
  let start = i;
  while (start > 0 && !isSpace(text[start - 1]!)) start -= 1;
  if (text[start] !== "/") return null;
  if (start > 0 && text[start - 1] === ":") return null;
  if (isMarkdownBlockquotePosition(text, start)) return null;

  let end = start + 1;
  while (end < text.length && !isSpace(text[end]!)) end += 1;

  const typed = text.slice(start + 1, i);
  if (typed.includes("/") || typed.includes("\\")) return null;
  if (native) {
    if (!/^[a-zA-Z0-9_.:-]*$/.test(typed)) return null;
  } else {
    if (/[A-Z]/.test(typed)) return null;
    if (!/^(?:[a-z0-9-]+(?::[a-z0-9-]*)?)?$/.test(typed)) return null;
  }

  return { start, end, query: typed };
}

export function replaceSlashToken(
  text: string,
  token: SlashToken,
  name: string,
): string {
  const rest = text.slice(token.end);
  const spacer = rest.startsWith(" ") ? "" : " ";
  return `${text.slice(0, token.start)}/${name}${spacer}${rest}`;
}

function isSpace(ch: string): boolean {
  return ch === " " || ch === "\n" || ch === "\t" || ch === "\r";
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}
