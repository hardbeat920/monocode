import {
  memo,
  useContext,
  useEffect,
  useMemo,
  useState,
  type CSSProperties,
  type ReactElement,
} from "react";
import type { BundledLanguage } from "shiki";
import type { HighlightResult } from "@streamdown/code";
import { CodeBlockContainer, CodeBlockHeader } from "streamdown";
import {
  boundedCode,
  requestCodeHighlight,
} from "../../files/editor/codeHighlightPlugin";
import { TranscriptVisibility } from "./transcriptVisibility";

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
  const visible = useContext(TranscriptVisibility);
  const [highlighted, setHighlighted] = useState<{
    language: string;
    source: string;
    result: HighlightResult;
  }>();
  useEffect(() => {
    if (
      !visible ||
      (highlighted?.language === language && highlighted.source === source)
    )
      return;
    return requestCodeHighlight(
      {
        code: source,
        language: language as BundledLanguage,
        themes: boundedCode.getThemes(),
      },
      (result) =>
        setHighlighted((previous) => {
          // Structured cloning makes fresh arrays. Reuse complete, unchanged
          // lines so streaming does not rebuild thousands of token elements.
          if (
            previous?.language === language &&
            source.startsWith(previous.source)
          ) {
            const completeLines =
              previous.source
                .slice(0, previous.source.lastIndexOf("\n") + 1)
                .split("\n").length - 1;
            result = {
              ...result,
              tokens: [
                ...previous.result.tokens.slice(0, completeLines),
                ...result.tokens.slice(completeLines),
              ],
            };
          }
          return { language, source, result };
        }),
      isIncomplete ? 0 : 1,
    );
  }, [source, language, visible, isIncomplete]);
  // New code is visible immediately, independently of tokenizer latency.
  const result = useMemo(() => {
    if (highlighted?.language !== language) return undefined;
    if (highlighted.source === source) return highlighted.result;
    if (!source.startsWith(highlighted.source)) return undefined;
    const boundary = highlighted.source.lastIndexOf("\n") + 1;
    const completeLines =
      highlighted.source.slice(0, boundary).split("\n").length - 1;
    return {
      ...highlighted.result,
      tokens: [
        ...highlighted.result.tokens.slice(0, completeLines),
        ...source
          .slice(boundary)
          .split("\n")
          .map((content) => [{ content, offset: 0 }]),
      ],
    };
  }, [highlighted, language, source]);
  return (
    <CodePresentation
      result={result}
      source={source}
      language={language}
      isIncomplete={isIncomplete}
      lineNumbers={lineNumbers}
      startLine={startLine}
    />
  );
}

const CodePresentation = memo(function CodePresentation({
  result,
  source,
  language,
  isIncomplete,
  lineNumbers,
  startLine,
}: {
  result?: HighlightResult;
  source: string;
  language: string;
  isIncomplete: boolean;
  lineNumbers: boolean;
  startLine?: number;
}) {
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
});
