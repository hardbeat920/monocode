import {
  bundledLanguages,
  bundledLanguagesInfo,
  createHighlighter,
  type BundledLanguage,
  type GrammarState,
} from "shiki";
import { createJavaScriptRegexEngine } from "shiki/engine/javascript";

/** The phone is dark only, so one theme, as the desktop's dark code blocks. */
const THEME = "github-dark";
/** Past this, highlighting at once would hold up the desktop window for too
 *  long; larger files are highlighted in the background instead. */
const MAX_HIGHLIGHT_CHARS = 120_000;
/** Longer lines (minified bundles, say) stay plain, as in the desktop editor. */
const MAX_LINE_LENGTH = 1_000;
/** A larger file is highlighted in the background this many lines at a time,
 *  the window getting a turn between chunks. */
const CHUNK_LINES = 400;
/** Background highlights kept for phones to collect, oldest dropped first. */
const MAX_JOBS = 4;

const aliases: Record<string, string> = Object.fromEntries(
  bundledLanguagesInfo.flatMap((info) => [
    [info.id, info.id],
    ...(info.aliases ?? []).map((alias) => [alias, info.id]),
  ]),
);
/** File names Shiki's aliases don't cover by extension. */
const BY_NAME: Record<string, string> = {
  dockerfile: "docker",
  makefile: "make",
  gemfile: "ruby",
  rakefile: "ruby",
  "cmakelists.txt": "cmake",
};
const BY_EXTENSION: Record<string, string> = {
  zsh: "shellscript",
  h: "c",
  hpp: "cpp",
  cc: "cpp",
  cxx: "cpp",
  kt: "kotlin",
  mdx: "mdx",
  lock: "json",
};

/** Shiki's language for a file, by its name, or undefined to leave it plain. */
export function languageForFile(path: string): BundledLanguage | undefined {
  const name = path.slice(path.lastIndexOf("/") + 1).toLowerCase();
  const extension = name.includes(".") ? name.slice(name.lastIndexOf(".") + 1) : "";
  const language =
    BY_NAME[name] ?? BY_EXTENSION[extension] ?? (extension ? aliases[extension] : undefined);
  return language && language in bundledLanguages ? (language as BundledLanguage) : undefined;
}

/**
 * A file's lines as runs of one colour, the colours sent once in `palette`
 * and each run as [text, index into it]. Neighbouring tokens of a colour are
 * merged, which keeps the message to a phone small.
 */
export type HighlightedLines = { palette: string[]; lines: [string, number][][] };

let highlighter: ReturnType<typeof createHighlighter> | undefined;

/** Recent results by path and text, since the phone asks for the same files
 *  again (Changes polls); bounded by entries and by total characters. */
const CACHE_ENTRIES = 60;
const CACHE_CHARS = 2_000_000;
const cache = new Map<string, HighlightedLines | undefined>();
let cachedChars = 0;

function remember(key: string, value: HighlightedLines | undefined) {
  if (cache.delete(key)) cachedChars -= key.length;
  cache.set(key, value);
  cachedChars += key.length;
  while (cache.size > CACHE_ENTRIES || cachedChars > CACHE_CHARS) {
    const oldest = cache.keys().next().value;
    if (oldest === undefined) break;
    cache.delete(oldest);
    cachedChars -= oldest.length;
  }
}

/**
 * Highlights a file for the phone's file viewer, or returns undefined when it
 * is too large, in a language Shiki doesn't know, or highlighting fails; the
 * phone then shows it plain.
 */
export async function highlightForMobile(
  path: string,
  text: string,
): Promise<HighlightedLines | undefined> {
  const language = languageForFile(path);
  if (!language || text.length > MAX_HIGHLIGHT_CHARS) return undefined;
  const key = `${language}\u0000${text}`;
  if (cache.has(key)) {
    const cached = cache.get(key);
    remember(key, cached);
    return cached;
  }
  try {
    highlighter ??= createHighlighter({
      themes: [THEME],
      langs: [],
      engine: createJavaScriptRegexEngine({ forgiving: true }),
    });
    const loaded = await highlighter;
    if (!loaded.getLoadedLanguages().includes(language)) await loaded.loadLanguage(language);
    const { tokens } = loaded.codeToTokens(text, {
      lang: language,
      theme: THEME,
      tokenizeMaxLineLength: MAX_LINE_LENGTH,
    });
    const palette: string[] = [];
    const colors = new Map<string, number>();
    const lines = tokens.map((line) => toRuns(line, palette, colors));
    remember(key, { palette, lines });
    return { palette, lines };
  } catch {
    highlighter = undefined;
    return undefined;
  }
}

type Job = {
  palette: string[];
  colors: Map<string, number>;
  lines: [string, number][][];
  done: boolean;
};

const jobs = new Map<string, Job>();
let nextJob = 0;

/**
 * Starts highlighting a file too large to highlight at once, in chunks with a
 * pause between them so the desktop window stays responsive. Each chunk picks
 * up the grammar where the last left it, so strings and comments spanning
 * lines colour as they do in the whole file. Returns the job's ID for
 * `highlightProgress`, or undefined for a language Shiki doesn't know.
 */
export function highlightInBackground(path: string, text: string): string | undefined {
  const language = languageForFile(path);
  if (!language) return undefined;
  const id = String(++nextJob);
  const job: Job = { palette: [], colors: new Map(), lines: [], done: false };
  jobs.set(id, job);
  while (jobs.size > MAX_JOBS) jobs.delete(jobs.keys().next().value!);
  void (async () => {
    try {
      highlighter ??= createHighlighter({
        themes: [THEME],
        langs: [],
        engine: createJavaScriptRegexEngine({ forgiving: true }),
      });
      const loaded = await highlighter;
      if (!loaded.getLoadedLanguages().includes(language)) await loaded.loadLanguage(language);
      const all = text.split("\n");
      let grammarState: GrammarState | undefined;
      for (let start = 0; start < all.length; start += CHUNK_LINES) {
        // A phone that stopped asking: the job was dropped, so stop working on it.
        if (!jobs.has(id)) return;
        const chunk = all.slice(start, start + CHUNK_LINES);
        const result = loaded.codeToTokens(chunk.join("\n"), {
          lang: language,
          theme: THEME,
          tokenizeMaxLineLength: MAX_LINE_LENGTH,
          ...(grammarState ? { grammarState } : {}),
        });
        grammarState = result.grammarState;
        for (const line of result.tokens) job.lines.push(toRuns(line, job.palette, job.colors));
        await new Promise((resolve) => setTimeout(resolve, 0));
      }
    } catch {
      // What was highlighted stays; the rest of the file stays plain.
    }
    job.done = true;
  })();
  return id;
}

/** A background highlight's palette and the lines finished from `from` on. */
export function highlightProgress(id: string, from: number) {
  const job = jobs.get(id);
  if (!job) return { palette: [], lines: [], done: true };
  return { palette: job.palette, lines: job.lines.slice(from), done: job.done };
}

/** A line's tokens as runs of one colour, adding new colours to the palette. */
function toRuns(
  line: { content: string; color?: string }[],
  palette: string[],
  colors: Map<string, number>,
) {
  const runs: [string, number][] = [];
  for (const token of line) {
    const color = token.color ?? "";
    let index = colors.get(color);
    if (index === undefined) {
      index = palette.push(color) - 1;
      colors.set(color, index);
    }
    const last = runs[runs.length - 1];
    if (last && last[1] === index) last[0] += token.content;
    else runs.push([token.content, index]);
  }
  return runs;
}

type DiffLineShape = {
  kind: "add" | "del" | "context" | "hunk";
  text: string;
  oldNumber: number | null;
  newNumber: number | null;
};

/**
 * A diff's lines with their syntax colours, for the phone's diff sheet. Both
 * versions of the file are highlighted whole, so strings and comments that
 * span lines colour as they do in the file, and each line takes its colours
 * from its side: removed lines from the old version, the rest from the new.
 * Undefined when neither side could be highlighted.
 */
export async function highlightDiffForMobile<T extends DiffLineShape>(
  path: string,
  original: string,
  current: string,
  diffLines: T[],
): Promise<{ palette: string[]; lines: (T & { runs?: [string, number][] })[] } | undefined> {
  const [before, after] = await Promise.all([
    highlightForMobile(path, original),
    highlightForMobile(path, current),
  ]);
  if (!before && !after) return undefined;
  // One palette for both sides.
  const palette = [...(after?.palette ?? [])];
  const index = new Map(palette.map((color, i) => [color, i]));
  const remap = (before?.palette ?? []).map((color) => {
    let i = index.get(color);
    if (i === undefined) {
      i = palette.push(color) - 1;
      index.set(color, i);
    }
    return i;
  });
  const lines = diffLines.map((line) => {
    const fromBefore = line.kind === "del" || (line.kind === "context" && !after);
    const runs =
      line.kind === "hunk"
        ? undefined
        : fromBefore
          ? before?.lines[(line.oldNumber ?? 0) - 1]?.map(
              ([text, color]): [string, number] => [text, remap[color] ?? 0],
            )
          : after?.lines[(line.newNumber ?? 0) - 1];
    // Only colours that spell out the line exactly; anything else stays plain.
    return runs && runs.map(([text]) => text).join("") === line.text ? { ...line, runs } : line;
  });
  return { palette, lines };
}
