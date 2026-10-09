import {
  memo,
  useEffect,
  useMemo,
  useState,
  type CSSProperties,
  type ReactElement,
} from "react";
import type { BundledLanguage } from "shiki";
import type { HighlightResult } from "@streamdown/code";
import { CodeBlockContainer, CodeBlockHeader } from "streamdown";
import { boundedCode } from "../../files/editor/codeHighlightPlugin";

type Tokens = HighlightResult["tokens"][number];

// Complete lines keep their token arrays while a fence grows. Memoizing here
// avoids recreating thousands of token elements on every reveal frame.
const HighlightedLine = memo(function HighlightedLine({
  tokens,
  numbered,
}: {
  tokens: Tokens;
  numbered: boolean;
}) {
  return (
    <span className={numbered ? "markdown-code-numbered-line" : undefined}>
      {tokens.length && !(tokens.length === 1 && tokens[0].content === "")
        ? tokens.map((token, index) => {
            const style: Record<string, string> = { ...token.htmlStyle };
            if (token.color) style["--sdm-c"] = token.color;
            if (token.bgColor) style["--sdm-tbg"] = token.bgColor;
            if (style.color) {
              style["--sdm-c"] = style.color;
              delete style.color;
            }
            if (style["background-color"]) {
              style["--sdm-tbg"] = style["background-color"];
              delete style["background-color"];
            }
            return (
              <span
                key={index}
                {...token.htmlAttrs}
                className="markdown-code-token"
                style={style as CSSProperties}
              >
                {token.content}
              </span>
            );
          })
        : "\n"}
    </span>
  );
});

const HighlightedBody = memo(function HighlightedBody({
  result,
  lineNumbers,
  startLine,
}: {
  result: HighlightResult;
  lineNumbers: boolean;
  startLine?: number;
}) {
  const cachedLines = useMemo(
    () =>
      new WeakMap<
        Tokens,
        { index: number; numbered: boolean; element: ReactElement }
      >(),
    [],
  );
  const lines = useMemo(() => {
    return result.tokens.map((tokens, index) => {
      const cached = cachedLines.get(tokens);
      if (cached?.index === index && cached.numbered === lineNumbers)
        return cached.element;
      const element = (
        <HighlightedLine key={index} tokens={tokens} numbered={lineNumbers} />
      );
      cachedLines.set(tokens, { index, numbered: lineNumbers, element });
      return element;
    });
  }, [result, lineNumbers, cachedLines]);
  return (
    <pre>
      <code
        style={
          lineNumbers
            ? { counterReset: `line ${(startLine ?? 1) - 1}` }
            : undefined
        }
      >
        {lines}
      </code>
    </pre>
  );
});

export function HighlightedCodeBlock({
  code,
  language,
  isIncomplete,
  lineNumbers,
  startLine,
}: {
  code: string;
  language: string;
  isIncomplete: boolean;
  lineNumbers: boolean;
  startLine?: number;
}) {
  const source = code.replace(/\n+$/, "");
  const [highlighted, setHighlighted] = useState<{
    language: string;
    result: HighlightResult;
  }>();
  useEffect(() => {
    let current = true;
    const apply = (result: HighlightResult) => {
      if (current) setHighlighted({ language, result });
    };
    const cached = boundedCode.highlight(
      {
        code: source,
        language: language as BundledLanguage,
        themes: boundedCode.getThemes(),
      },
      apply,
    );
    if (cached) apply(cached);
    return () => {
      current = false;
    };
  }, [source, language]);
  const result =
    highlighted?.language === language ? highlighted.result : undefined;
  return (
    <CodeBlockContainer language={language} isIncomplete={isIncomplete}>
      <CodeBlockHeader language={language} />
      <div data-streamdown="code-block-body" data-language={language}>
        {result ? (
          <HighlightedBody
            result={result}
            lineNumbers={lineNumbers}
            startLine={startLine}
          />
        ) : (
          <pre>
            <code
              style={
                lineNumbers
                  ? { counterReset: `line ${(startLine ?? 1) - 1}` }
                  : undefined
              }
            >
              {source.split("\n").map((line, index) => (
                <span
                  key={index}
                  className={
                    lineNumbers ? "markdown-code-numbered-line" : undefined
                  }
                >
                  {line || "\n"}
                </span>
              ))}
            </code>
          </pre>
        )}
      </div>
    </CodeBlockContainer>
  );
}
