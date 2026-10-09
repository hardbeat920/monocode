import type {
  CodeHighlighterPlugin,
  HighlightResult,
  ThemeInput,
} from "@streamdown/code";
import {
  bundledLanguages,
  bundledLanguagesInfo,
  createHighlighter,
  type BundledLanguage,
  type GrammarState,
} from "shiki";
import { createJavaScriptRegexEngine } from "shiki/engine/javascript";

/**
 * `@streamdown/code` keeps every token result in a module-level Map that is
 * never evicted. Streamdown highlights a fence again on every streamed update,
 * so each partial version of every code block an agent wrote stayed in memory
 * for the life of the window. Its key (length plus the first and last 100
 * characters) also let different blocks share a result. This keeps the same
 * `CodeHighlighterPlugin` shape with a bounded LRU and an exact key, and loads
 * languages into one highlighter per theme pair instead of one per language.
 *
 * Mounted blocks hold their tokens in component state, so eviction only means
 * a remounted block briefly shows plain text while it is highlighted again.
 */
const DEFAULT_THEMES: [ThemeInput, ThemeInput] = ["github-light", "github-dark"];
const MAX_ENTRIES = 100;
const MAX_CACHED_CHARS = 500_000;

type Highlighter = Awaited<ReturnType<typeof createHighlighter>>;
type HighlightCallback = (result: HighlightResult) => void;

export type BoundedCodePlugin = CodeHighlighterPlugin & {
  /** Number of token results currently cached. */
  cachedResults(): number;
};

const engine = createJavaScriptRegexEngine({ forgiving: true });
const aliases: Record<string, string> = Object.fromEntries(
  bundledLanguagesInfo.flatMap((info) =>
    (info.aliases ?? []).map((alias) => [alias, info.id]),
  ),
);
const supported = new Set(Object.keys(bundledLanguages));

function normalizeLanguage(language: string): string {
  const lower = language.trim().toLowerCase();
  return aliases[lower] ?? lower;
}

function themeName(theme: ThemeInput): string {
  return typeof theme === "string" ? theme : (theme.name ?? "custom");
}

export function createBoundedCodePlugin(
  options: {
    themes?: [ThemeInput, ThemeInput];
    maxEntries?: number;
    maxChars?: number;
  } = {},
): BoundedCodePlugin {
  const themes = options.themes ?? DEFAULT_THEMES;
  const maxEntries = options.maxEntries ?? MAX_ENTRIES;
  const maxChars = options.maxChars ?? MAX_CACHED_CHARS;
  const highlighters = new Map<string, Promise<Highlighter>>();
  const results = new Map<string, HighlightResult>();
  const pending = new Map<string, Set<HighlightCallback>>();
  let cachedChars = 0;
  // Only the most recently highlighted fence needs a streaming checkpoint.
  // Keep complete lines and their grammar state; the unfinished line must be
  // tokenized again because its syntax can change as characters arrive.
  let checkpoint:
    | {
        config: string;
        source: string;
        tokens: HighlightResult["tokens"];
        state: GrammarState | undefined;
      }
    | undefined;

  const highlighterFor = (pair: [ThemeInput, ThemeInput]) => {
    const key = `${themeName(pair[0])}\u0000${themeName(pair[1])}`;
    let highlighter = highlighters.get(key);
    if (!highlighter) {
      highlighter = createHighlighter({ themes: pair, langs: [], engine });
      highlighters.set(key, highlighter);
      highlighter.catch(() => highlighters.delete(key));
    }
    return highlighter;
  };

  const remember = (key: string, result: HighlightResult) => {
    if (results.delete(key)) cachedChars -= key.length;
    // A block larger than the whole budget is delivered but never cached.
    if (key.length > maxChars) return;
    results.set(key, result);
    cachedChars += key.length;
    while (results.size > maxEntries || cachedChars > maxChars) {
      const oldest = results.keys().next().value;
      if (oldest === undefined) break;
      results.delete(oldest);
      cachedChars -= oldest.length;
    }
  };

  return {
    name: "shiki",
    type: "code-highlighter",
    supportsLanguage: (language) => supported.has(normalizeLanguage(language)),
    getSupportedLanguages: () => Array.from(supported) as BundledLanguage[],
    getThemes: () => themes,
    cachedResults: () => results.size,
    highlight({ code, language, themes: pair }, callback) {
      const lang = normalizeLanguage(language);
      const names = [themeName(pair[0]), themeName(pair[1])] as const;
      const key = `${lang}\u0000${names[0]}\u0000${names[1]}\u0000${code}`;
      const cached = results.get(key);
      if (cached) {
        // Refresh recency so blocks still on screen are evicted last.
        results.delete(key);
        results.set(key, cached);
        return cached;
      }
      const waiting = pending.get(key);
      if (waiting) {
        if (callback) waiting.add(callback);
        return null;
      }
      pending.set(key, new Set(callback ? [callback] : []));
      void highlighterFor(pair)
        .then(async (highlighter) => {
          if (supported.has(lang) && !highlighter.getLoadedLanguages().includes(lang)) {
            await highlighter.loadLanguage(lang as BundledLanguage);
          }
          const usable = highlighter.getLoadedLanguages().includes(lang)
            ? lang
            : "text";
          const config = `${usable}\u0000${names[0]}\u0000${names[1]}`;
          const previous =
            checkpoint?.config === config && code.startsWith(checkpoint.source)
              ? checkpoint
              : undefined;
          const offset = previous?.source.length ?? 0;
          const boundary = code.lastIndexOf("\n") + 1;
          const settings = {
            lang: usable as BundledLanguage,
            themes: { light: names[0], dark: names[1] },
          };
          let prefix = previous?.tokens ?? [];
          let state = previous?.state;
          if (boundary > offset) {
            const lineEnd =
              code[boundary - 2] === "\r" ? boundary - 2 : boundary - 1;
            const complete = highlighter.codeToTokens(
              code.slice(offset, lineEnd),
              {
                ...settings,
                grammarState: state,
              },
            );
            prefix = [
              ...prefix,
              ...complete.tokens.map((line) =>
                line.map((token) => ({
                  ...token,
                  offset: token.offset + offset,
                })),
              ),
            ];
            state = complete.grammarState;
          }
          const tail = highlighter.codeToTokens(code.slice(boundary), {
            ...settings,
            grammarState: state,
          });
          const result = {
            ...tail,
            tokens: [
              ...prefix,
              ...tail.tokens.map((line) =>
                line.map((token) => ({
                  ...token,
                  offset: token.offset + boundary,
                })),
              ),
            ],
          };
          checkpoint =
            boundary > 0 && boundary <= maxChars
              ? {
                  config,
                  source: code.slice(0, boundary),
                  tokens: prefix,
                  state,
                }
              : undefined;
          remember(key, result);
          const callbacks = pending.get(key);
          pending.delete(key);
          callbacks?.forEach((notify) => notify(result));
        })
        .catch((error: unknown) => {
          pending.delete(key);
          console.error("[Code highlight] Failed to highlight code:", error);
        });
      return null;
    },
  };
}

export const boundedCode = createBoundedCodePlugin();
