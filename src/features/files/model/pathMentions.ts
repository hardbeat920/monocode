import type { FsEntry } from "../../../platform/tauri/fs";
import { fuzzyMatch } from "../../../shared/lib/fuzzy";
import { joinPath } from "../../../shared/lib/paths";
import type { RankedFile } from "./fileIndex";

/** `@../`, `@~/` or `@/` — browse the disk instead of the project index. */
export type PathQuery = {
  /** Typed directory part, kept verbatim for the label (`../other/`). */
  dir: string;
  /** Name being typed inside `dir`. */
  partial: string;
};

const MAX_PICKER = 30;

export function pathMentionQuery(query: string): PathQuery | null {
  if (!/^(?:\.\.\/|~\/|\/)/.test(query)) return null;
  const slash = query.lastIndexOf("/");
  return { dir: query.slice(0, slash + 1), partial: query.slice(slash + 1) };
}

/**
 * Directory to list for a typed `dir`. `~/` is left for `list_dir` to expand,
 * relative parts are joined onto `cwd` (needed only for `../`).
 */
export function pathMentionDir(dir: string, cwd: string): string | null {
  if (dir.startsWith("~/") || dir.startsWith("/")) return dir;
  if (!cwd || cwd === "~") return null;
  return joinPath(cwd, dir);
}

/** Entries of one listed directory, labelled with the typed path. */
export function rankPathEntries(
  query: PathQuery,
  entries: FsEntry[],
  limit = MAX_PICKER,
): RankedFile[] {
  const needle = query.partial.trim();
  const out: RankedFile[] = [];
  for (const entry of entries) {
    if (!isPathTokenSafe(entry.name)) continue;
    // Dotfiles stay out of the way until a leading `.` asks for them.
    if (entry.name.startsWith(".") && !needle.startsWith(".")) continue;
    const hit = needle ? fuzzyMatch(needle, entry.name) : { score: 0, positions: [] };
    if (!hit) continue;
    out.push({
      name: entry.name,
      path: entry.path,
      relative: `${query.dir}${entry.name}`,
      isDir: entry.isDir,
      score: hit.score - (entry.ignored ? 200 : 0),
      positions: hit.positions.map((pos) => pos + query.dir.length),
    });
  }
  if (needle) {
    // Stable sort keeps `list_dir`'s folders-first natural order on ties.
    out.sort((a, b) => b.score - a.score);
  }
  return out.slice(0, limit);
}

/** A path label must survive as one whitespace-delimited `@` token. */
function isPathTokenSafe(name: string): boolean {
  return (
    name.length > 0 &&
    !/[\s@\\]/.test(name) &&
    !/[\p{Cc}\p{Cf}\p{Zl}\p{Zp}]/u.test(name)
  );
}
