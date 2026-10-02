import type { HighlightResult } from "@streamdown/code";
import * as shiki from "shiki";
import type { BundledLanguage } from "shiki";
import { createJavaScriptRegexEngine } from "shiki/engine/javascript";
import { describe, expect, it, vi } from "vitest";
import {
  createBoundedCodePlugin,
  type BoundedCodePlugin,
} from "./codeHighlightPlugin";

vi.mock("shiki", async (importOriginal) => {
  const actual = await importOriginal<typeof import("shiki")>();
  return { ...actual, createHighlighter: vi.fn(actual.createHighlighter) };
});

function highlight(
  plugin: BoundedCodePlugin,
  code: string,
  language = "ts",
): Promise<HighlightResult> {
  return new Promise((resolve) => {
    const cached = plugin.highlight(
      { code, language: language as BundledLanguage, themes: plugin.getThemes() },
      resolve,
    );
    if (cached) resolve(cached);
  });
}

function text(result: HighlightResult): string {
  return result.tokens
    .map((line) => line.map((token) => token.content).join(""))
    .join("\n");
}

describe("bounded code highlight plugin", () => {
  it("shares one highlight run across concurrent requests and notifies every callback", async () => {
    const plugin = createBoundedCodePlugin();
    const source = "const shared = 1;";
    const highlighter = await shiki.createHighlighter({
      themes: plugin.getThemes(),
      langs: [],
      engine: createJavaScriptRegexEngine({ forgiving: true }),
    });
    const createHighlighter = vi
      .mocked(shiki.createHighlighter)
      .mockClear()
      .mockResolvedValueOnce(highlighter);
    const codeToTokens = vi.spyOn(highlighter, "codeToTokens");

    try {
      const callbacks = [vi.fn(), vi.fn(), vi.fn()];
      const requests = callbacks.map((callback) =>
        new Promise<HighlightResult>((resolve) => {
          const cached = plugin.highlight(
            { code: source, language: "ts", themes: plugin.getThemes() },
            (result) => {
              callback(result);
              resolve(result);
            },
          );
          expect(cached).toBeNull();
        }),
      );
      const results = await Promise.all(requests);

      expect(createHighlighter).toHaveBeenCalledTimes(1);
      expect(codeToTokens).toHaveBeenCalledTimes(1);
      expect(text(results[0])).toBe(source);
      for (let i = 0; i < callbacks.length; i += 1) {
        expect(callbacks[i]).toHaveBeenCalledTimes(1);
        expect(callbacks[i]).toHaveBeenCalledWith(results[0]);
        expect(results[i]).toBe(results[0]);
      }
      expect(plugin.cachedResults()).toBe(1);
      expect(
        plugin.highlight({
          code: source,
          language: "ts",
          themes: plugin.getThemes(),
        }),
      ).toBe(results[0]);
      expect(codeToTokens).toHaveBeenCalledTimes(1);
    } finally {
      createHighlighter.mockReset();
      codeToTokens.mockRestore();
      highlighter.dispose();
    }
  }, 20_000);

  it("refreshes recency on a cache hit and rehighlights the least recently used entry", async () => {
    const plugin = createBoundedCodePlugin({ maxEntries: 2 });
    const first = "const first = 1;";
    const second = "const second = 2;";
    const third = "const third = 3;";
    const cached = (code: string) =>
      plugin.highlight({ code, language: "ts", themes: plugin.getThemes() });
    const firstResult = await highlight(plugin, first);
    const secondResult = await highlight(plugin, second);

    expect(cached(first)).toBe(firstResult);
    const thirdResult = await highlight(plugin, third);

    expect(plugin.cachedResults()).toBe(2);
    expect(cached(first)).toBe(firstResult);
    expect(cached(third)).toBe(thirdResult);
    expect(cached(second)).toBeNull();
    const rehighlighted = await highlight(plugin, second);
    expect(text(rehighlighted)).toBe(second);
    expect(rehighlighted).not.toBe(secondResult);
    expect(cached(second)).toBe(rehighlighted);
    expect(plugin.cachedResults()).toBe(2);
  }, 20_000);

  it("keeps the cache bounded while a fence streams in", async () => {
    const plugin = createBoundedCodePlugin({ maxEntries: 8 });
    const source = Array.from(
      { length: 40 },
      (_, i) => `const value${i} = ${i};`,
    ).join("\n");
    // Streamdown highlights again on every update of an open fence.
    for (let end = 20; end <= source.length; end += 20) {
      await highlight(plugin, source.slice(0, end));
    }
    expect(plugin.cachedResults()).toBeLessThanOrEqual(8);
    expect(text(await highlight(plugin, source))).toBe(source);
  }, 20_000);

  it("caps the cached source size", async () => {
    const plugin = createBoundedCodePlugin({ maxChars: 2_000 });
    for (let i = 0; i < 5; i += 1) {
      await highlight(plugin, `// block ${i}\n${"x".repeat(900)}`);
    }
    expect(plugin.cachedResults()).toBe(2);
  }, 20_000);

  it("delivers but does not cache a block larger than the budget", async () => {
    const plugin = createBoundedCodePlugin({ maxChars: 500 });
    const huge = `// big\n${"y".repeat(1_000)}`;
    expect(text(await highlight(plugin, huge))).toBe(huge);
    expect(plugin.cachedResults()).toBe(0);
  }, 20_000);

  it("returns cached tokens synchronously and keeps distinct blocks apart", async () => {
    const plugin = createBoundedCodePlugin();
    const head = "const a = 1;\n".repeat(10);
    const tail = "\nconst z = 26;".repeat(10);
    const first = `${head}let middle = "one";${tail}`;
    const second = `${head}let middle = "two";${tail}`;
    expect(first.length).toBe(second.length);

    expect(text(await highlight(plugin, first))).toBe(first);
    expect(text(await highlight(plugin, second))).toBe(second);
    const again = plugin.highlight(
      { code: first, language: "ts" as BundledLanguage, themes: plugin.getThemes() },
      () => undefined,
    );
    expect(again && text(again)).toBe(first);
  }, 20_000);

  it("colors code and falls back to plain text for unknown languages", async () => {
    const plugin = createBoundedCodePlugin();
    const colored = await highlight(plugin, "const x = 1;", "typescript");
    const colors = new Set(
      colored.tokens.flat().map((token) => token.htmlStyle?.color ?? token.color),
    );
    expect(colors.size).toBeGreaterThan(1);
    const plain = await highlight(plugin, "anything", "not-a-language");
    expect(text(plain)).toBe("anything");
  }, 20_000);
});
