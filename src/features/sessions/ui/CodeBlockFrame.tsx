import {
  createContext,
  useContext,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import type { Element, Root } from "hast";
import { copyText } from "../../../platform/tauri/clipboard";
import { TextWrap } from "../../../shared/ui/icons";

const WRAPPED_LANGUAGES = new Set([
  "markdown",
  "md",
  "mdx",
  "text",
  "plaintext",
  "plain",
  "txt",
]);

/** Use the original fence label/file, not its substituted highlighting grammar. */
export function wrapCodeByDefault(
  language: string,
  fileName?: string,
): boolean {
  const name = (fileName ?? language)
    .toLowerCase()
    .replace(/(?::\d+(?::\d+)?|#L\d+(?:-L\d+)?)$/i, "");
  return WRAPPED_LANGUAGES.has(name.slice(name.lastIndexOf(".") + 1));
}

/** Fence order stays stable when preceding prose or source lines grow. */
export function rehypeCodeWrapIndices() {
  return (tree: Root) => {
    let index = 0;
    function walk(parent: Root | Element) {
      for (const child of parent.children) {
        if (child.type !== "element") continue;
        if (
          parent.type === "element" &&
          parent.tagName === "pre" &&
          child.tagName === "code"
        ) {
          child.properties.dataWrapIndex = index++;
        } else {
          walk(child);
        }
      }
    }
    walk(tree);
  };
}

// A parsed Markdown block owns this map. It keeps nested choices across the
// remount that removes prose fade spans, without relying on shifting offsets.
export type CodeWrapOverrides = Map<number, boolean>;
export const CodeWrapOverridesContext = createContext<CodeWrapOverrides | null>(
  null,
);

export function CodeBlockFrame({
  code,
  defaultWrapped,
  blockIndex,
  children,
}: {
  code: string;
  defaultWrapped: boolean;
  blockIndex?: number;
  children: ReactNode;
}) {
  const overrides = useContext(CodeWrapOverridesContext);
  const [override, setOverride] = useState<boolean | undefined>(() =>
    blockIndex === undefined ? undefined : overrides?.get(blockIndex),
  );
  const wrapped = override ?? defaultWrapped;

  return (
    <div className="markdown-code-shell" dir="ltr" data-wrap={wrapped}>
      <div className="markdown-code-actions">
        <button
          type="button"
          className="markdown-code-action markdown-code-wrap"
          aria-label="Wrap lines"
          aria-pressed={wrapped}
          title={wrapped ? "Disable line wrapping" : "Enable line wrapping"}
          onClick={() => {
            if (blockIndex !== undefined) overrides?.set(blockIndex, !wrapped);
            setOverride(!wrapped);
          }}
        >
          <TextWrap aria-hidden="true" size={15} />
        </button>
        <CodeCopyButton code={code} />
      </div>
      {children}
    </div>
  );
}

function CodeCopyButton({ code }: { code: string }) {
  const [copied, setCopied] = useState(false);
  const timer = useRef<number | null>(null);

  useEffect(() => {
    setCopied(false);
    return () => {
      if (timer.current != null) window.clearTimeout(timer.current);
    };
  }, [code]);

  return (
    <button
      type="button"
      title={copied ? "Copied" : "Copy code"}
      aria-label={copied ? "Copied" : "Copy code"}
      className={`markdown-code-action markdown-code-copy ${copied ? "is-copied" : ""}`}
      onClick={() => {
        void copyText(code.replace(/\r?\n$/, "")).then(
          () => {
            setCopied(true);
            if (timer.current != null) window.clearTimeout(timer.current);
            timer.current = window.setTimeout(() => setCopied(false), 1500);
          },
          () => {},
        );
      }}
    >
      <svg viewBox="0 0 20 20" aria-hidden="true">
        <g className="markdown-code-copy-pages">
          <path d="M12 4H6a2 2 0 0 0-2 2v6" />
          <rect x="7" y="7" width="9" height="9" rx="1.5" />
        </g>
        <path
          className="markdown-code-copy-check"
          d="M4.5 10.5 8.2 14 15.5 6.5"
        />
      </svg>
    </button>
  );
}
