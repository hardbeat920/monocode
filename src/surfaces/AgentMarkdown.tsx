import { code } from "@streamdown/code";
import {
  createContext,
  isValidElement,
  memo,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ComponentProps,
  type ReactNode,
} from "react";
import { harden } from "rehype-harden";
import {
  CodeBlock,
  Streamdown,
  defaultRehypePlugins,
  useIsCodeFenceIncomplete,
  type Components,
} from "streamdown";
import type { PluggableList } from "unified";
import { FileTypeIcon } from "../chrome/FileTypeIcon";
import { createLazyMermaidPlugin } from "./mermaidPlugin";
import { resolveWorkspacePath } from "../lib/paths";
import { isAtxHeadingLine } from "../lib/markdownSource";
import { useColorScheme } from "../hooks/useColorScheme";
import { useLockOverscroll } from "../hooks/useLockOverscroll";
import { copyText } from "../lib/clipboard";
import { INBOX_MEDIA_PREFIXES, isInboxMediaUrl } from "../lib/inboxMedia";
import { InboxMedia } from "./InboxMedia";

const MERMAID_BASE_CONFIG = {
  startOnLoad: false,
  securityLevel: "strict",
  suppressErrorRendering: true,
} as const;

const mermaid = createLazyMermaidPlugin({
  config: {
    ...MERMAID_BASE_CONFIG,
    theme: "dark",
  },
});

const MARKDOWN_PLUGINS = { code, mermaid };

const MARKDOWN_REHYPE_PLUGINS: PluggableList = [
  defaultRehypePlugins.raw,
  defaultRehypePlugins.sanitize,
  [
    harden,
    {
      allowedImagePrefixes: [] as string[],
      allowedLinkPrefixes: ["*"],
      allowDataImages: true,
      imageBlockPolicy: "remove" as const,
    },
  ],
];

const INBOX_MEDIA_REHYPE_PLUGINS: PluggableList = [
  defaultRehypePlugins.raw,
  defaultRehypePlugins.sanitize,
  [
    harden,
    {
      defaultOrigin: "https://inbox.invalid",
      allowedImagePrefixes: INBOX_MEDIA_PREFIXES,
      allowedLinkPrefixes: ["*"],
      allowDataImages: true,
      imageBlockPolicy: "remove" as const,
    },
  ],
];

const FileOpenContext = createContext<{
  cwd?: string;
  onOpenFile?: (path: string) => void;
}>({});

const RemoteMediaContext = createContext(false);

const LANGUAGE_FROM_EXT: Record<string, string> = {
  sh: "bash",
  zsh: "bash",
  py: "python",
  rb: "ruby",
  rs: "rust",
  ts: "typescript",
  js: "javascript",
  md: "markdown",
  yml: "yaml",
  cs: "csharp",
  cpp: "cpp",
  cc: "cpp",
  cxx: "cpp",
};

const LANGUAGE_FILE_NAMES: Record<string, string> = {
  bash: "code.sh",
  c: "code.c",
  cpp: "code.cpp",
  "c++": "code.cpp",
  csharp: "code.cs",
  css: "code.css",
  go: "code.go",
  html: "code.html",
  java: "code.java",
  javascript: "code.js",
  js: "code.js",
  jsx: "code.jsx",
  json: "code.json",
  markdown: "code.md",
  md: "code.md",
  php: "code.php",
  python: "code.py",
  py: "code.py",
  ruby: "code.rb",
  rust: "code.rs",
  rs: "code.rs",
  shell: "code.sh",
  sh: "code.sh",
  sql: "code.sql",
  swift: "code.swift",
  toml: "code.toml",
  ts: "code.ts",
  tsx: "code.tsx",
  typescript: "code.ts",
  xml: "code.xml",
  yaml: "code.yaml",
  yml: "code.yaml",
  zsh: "code.sh",
};

type MarkdownLinkProps = ComponentProps<"a"> & { node?: unknown };

function MarkdownLink({
  href,
  children,
  className,
  node: _node,
  onClick,
  dir,
  ...props
}: MarkdownLinkProps) {
  const allowRemoteMedia = useContext(RemoteMediaContext);
  const { cwd, onOpenFile } = useContext(FileOpenContext);
  const filePath = href ? resolveWorkspacePath(href, cwd) : undefined;
  const label = textContent(children);
  if (allowRemoteMedia && href && isInboxMediaUrl(href)) {
    return <InboxMedia src={href} alt={label} />;
  }

  return (
    <a
      href={href}
      className={`text-sky-400/90 hover:text-sky-300 hover:underline ${className ?? ""}`}
      {...props}
      dir={dir ?? "auto"}
      onClick={(event) => {
        onClick?.(event);
        if (event.defaultPrevented) return;
        if (filePath && onOpenFile) {
          event.preventDefault();
          onOpenFile(filePath);
          return;
        }
        if (!href || !/^https?:\/\//i.test(href)) {
          event.preventDefault();
        }
      }}
    >
      {children}
    </a>
  );
}

type MarkdownCodeProps = ComponentProps<"code"> & { node?: unknown };

function MarkdownCode({
  children,
  className,
  node,
  ...props
}: MarkdownCodeProps) {
  const incomplete = useIsCodeFenceIncomplete();
  const block = Object.prototype.hasOwnProperty.call(props, "data-block");
  if (!block) {
    const text = textContent(children);
    const fileName = inlineFileName(text);
    const { cwd, onOpenFile } = useContext(FileOpenContext);
    const filePath = fileName ? resolveWorkspacePath(text, cwd) : undefined;
    const open =
      filePath && onOpenFile ? () => onOpenFile(filePath) : undefined;
    return (
      <code
        {...props}
        dir="ltr"
        className={`inline-flex items-center gap-1 rounded-md bg-content/8 px-1.5 h-6 align-baseline font-mono text-[0.8em] text-content ${
          open ? "cursor-pointer hover:text-sky-300 hover:underline" : ""
        } ${className ?? ""}`}
        role={open ? "link" : undefined}
        tabIndex={open ? 0 : undefined}
        onClick={open}
        onKeyDown={
          open
            ? (event) => {
                if (event.key === "Enter" || event.key === " ") {
                  event.preventDefault();
                  open();
                }
              }
            : undefined
        }
      >
        {fileName ? (
          <span aria-hidden="true">
            <FileTypeIcon name={fileName} isDir={false} size={14} />
          </span>
        ) : null}
        {children}
      </code>
    );
  }

  const meta = codeMeta(node);
  const fence = parseCodeFence(className, meta);
  if (fence.language.toLowerCase() === "mermaid") {
    return (
      <MermaidBlock code={textContent(children)} incomplete={incomplete} />
    );
  }
  const iconName =
    fence.fileName ??
    (fence.language ? fileNameForLanguage(fence.language) : "");
  const lineNumbers = !/\bnoLineNumbers\b/.test(meta);
  const code = textContent(children);

  return (
    <div className="markdown-code-shell" dir="ltr">
      {iconName ? (
        <span className="markdown-code-icon" aria-hidden="true">
          <FileTypeIcon name={iconName} isDir={false} />
        </span>
      ) : null}
      {fence.filePath ? <MarkdownCodePath path={fence.filePath} /> : null}
      <CodeCopyButton code={code} />
      <CodeBlock
        className={className}
        code={code}
        isIncomplete={incomplete}
        language={fence.language}
        lineNumbers={lineNumbers}
        startLine={fence.startLine}
      />
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
      className={`markdown-code-copy ${copied ? "is-copied" : ""}`}
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

type MarkdownImageProps = ComponentProps<"img"> & { node?: unknown };

function MarkdownImage({
  src,
  alt,
  node: _node,
  ...props
}: MarkdownImageProps) {
  const allowRemoteMedia = useContext(RemoteMediaContext);
  const url = typeof src === "string" ? src.trim() : "";
  if (url.startsWith("data:image/")) {
    return <img {...props} src={url} alt={alt ?? ""} />;
  }
  if (!allowRemoteMedia || !url || !isInboxMediaUrl(url)) return null;
  return <InboxMedia src={url} alt={alt} />;
}

const MARKDOWN_COMPONENTS = {
  a: MarkdownLink,
  code: MarkdownCode,
  img: MarkdownImage,
} satisfies Components;

export const AgentMarkdown = memo(function AgentMarkdown({
  text,
  streaming,
  className,
  cwd,
  onOpenFile,
  allowRemoteMedia,
}: {
  text: string;
  streaming?: boolean;
  className?: string;
  cwd?: string;
  onOpenFile?: (path: string) => void;
  allowRemoteMedia?: boolean;
}) {
  const fileOpen = useMemo(() => ({ cwd, onOpenFile }), [cwd, onOpenFile]);
  const remoteMedia = !!allowRemoteMedia;
  return (
    <RemoteMediaContext.Provider value={remoteMedia}>
      <FileOpenContext.Provider value={fileOpen}>
        <Streamdown
          className={`agent-markdown min-w-0 font-sans text-sm leading-6 ${className ?? ""}`}
          components={MARKDOWN_COMPONENTS}
          controls={false}
          dir="auto"
          isAnimating={!!streaming}
          plugins={MARKDOWN_PLUGINS}
          rehypePlugins={
            remoteMedia ? INBOX_MEDIA_REHYPE_PLUGINS : MARKDOWN_REHYPE_PLUGINS
          }
        >
          {text}
        </Streamdown>
      </FileOpenContext.Provider>
    </RemoteMediaContext.Provider>
  );
});

export const MarkdownPreview = memo(function MarkdownPreview({
  text,
  streaming,
  cwd,
  onOpenFile,
}: {
  text: string;
  streaming?: boolean;
  cwd?: string;
  onOpenFile?: (path: string) => void;
}) {
  const lockOverscroll = useLockOverscroll<HTMLDivElement>();

  return (
    <div
      ref={lockOverscroll}
      className="markdown-preview h-full overflow-y-auto overscroll-none [overflow-anchor:none]"
    >
      <div className="px-6 py-8">
        <AgentMarkdown
          text={text}
          streaming={streaming}
          cwd={cwd}
          onOpenFile={onOpenFile}
        />
      </div>
    </div>
  );
});

export const MarkdownSource = memo(function MarkdownSource({
  text,
}: {
  text: string;
}) {
  const lockOverscroll = useLockOverscroll<HTMLDivElement>();

  return (
    <div
      ref={lockOverscroll}
      className="markdown-preview h-full overflow-y-auto overscroll-none [overflow-anchor:none]"
    >
      <pre className="min-h-full min-w-0 whitespace-pre-wrap wrap-break-word px-4 py-3 font-mono text-[13px] leading-5 text-content/85">
        <MarkdownSourceHighlight text={text} />
      </pre>
    </div>
  );
});

export function MarkdownSourceHighlight({ text }: { text: string }) {
  if (!text) return null;
  return (
    <>
      {text.split(/(\n)/).map((part, index) =>
        part === "\n" ? (
          "\n"
        ) : isAtxHeadingLine(part) ? (
          <span key={index} className="markdown-source-heading">
            {part}
          </span>
        ) : (
          <span key={index}>{part}</span>
        ),
      )}
    </>
  );
}

function MermaidBlock({
  code,
  incomplete,
}: {
  code: string;
  incomplete: boolean;
}) {
  const [svg, setSvg] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);
  const colorScheme = useColorScheme();

  useEffect(() => {
    if (incomplete) {
      setSvg(null);
      setFailed(false);
      return;
    }
    let cancelled = false;
    setSvg(null);
    setFailed(false);
    const id = `mermaid-${Math.abs(hashCode(code)).toString(36)}-${Date.now().toString(36)}`;
    void mermaid
      .getMermaid({
        ...MERMAID_BASE_CONFIG,
        theme: colorScheme === "light" ? "default" : "dark",
      })
      .render(id, code)
      .then((result) => {
        if (cancelled) return;
        setSvg(result.svg);
      })
      .catch(() => {
        if (!cancelled) setFailed(true);
      });
    return () => {
      cancelled = true;
    };
  }, [code, incomplete, colorScheme]);

  if (incomplete || failed) {
    return (
      <div className="markdown-code-shell" dir="ltr">
        <span className="markdown-code-icon" aria-hidden="true">
          <FileTypeIcon name="diagram.mmd" isDir={false} />
        </span>
        <CodeCopyButton code={code} />
        <CodeBlock
          code={code}
          isIncomplete={incomplete}
          language="mermaid"
          lineNumbers={false}
        />
      </div>
    );
  }

  if (!svg) {
    return (
      <div className="h-32 animate-pulse rounded-[10px] border border-content/10 bg-content/6" />
    );
  }

  return (
    <div
      className="mermaid-block overflow-x-auto rounded-[10px] border border-content/10 bg-content/6 p-3"
      data-streamdown="mermaid-block"
      dir="ltr"
      dangerouslySetInnerHTML={{ __html: svg }}
    />
  );
}

function hashCode(value: string): number {
  let hash = 0;
  for (let i = 0; i < value.length; i++) {
    hash = (hash << 5) - hash + value.charCodeAt(i);
    hash |= 0;
  }
  return hash;
}

function codeMeta(node: unknown): string {
  if (!node || typeof node !== "object" || !("properties" in node)) return "";
  const properties = node.properties;
  if (
    !properties ||
    typeof properties !== "object" ||
    !("metastring" in properties)
  ) {
    return "";
  }
  return typeof properties.metastring === "string" ? properties.metastring : "";
}

function textContent(value: ReactNode): string {
  if (typeof value === "string" || typeof value === "number")
    return String(value);
  if (Array.isArray(value)) return value.map(textContent).join("");
  if (isValidElement<{ children?: ReactNode }>(value)) {
    return textContent(value.props.children);
  }
  return "";
}

function parseCodeFence(
  className: string | undefined,
  meta: string,
): {
  language: string;
  startLine?: number;
  fileName?: string;
  filePath?: string;
} {
  const raw = className?.match(/\blanguage-([^\s]+)/)?.[1] ?? "";
  const metaStart = meta.match(/\bstartLine=(\d+)/);
  const metaStartLine = metaStart ? Number(metaStart[1]) : undefined;

  const citation = raw.match(/^(\d+):(\d+):(.+)$/);
  if (citation) {
    const filePath = citation[3];
    const fileName = filePath.split(/[/\\]/).filter(Boolean).pop() ?? filePath;
    return {
      language: languageFromFileName(fileName),
      startLine: Number(citation[1]),
      fileName,
      filePath,
    };
  }

  if (/[/\\]/.test(raw)) {
    const fileName = raw.split(/[/\\]/).filter(Boolean).pop() ?? raw;
    return {
      language: languageFromFileName(fileName),
      startLine: metaStartLine,
      fileName,
      filePath: raw,
    };
  }

  return { language: raw, startLine: metaStartLine };
}

function MarkdownCodePath({ path }: { path: string }) {
  const { cwd, onOpenFile } = useContext(FileOpenContext);
  const filePath = resolveWorkspacePath(path, cwd);
  if (!filePath || !onOpenFile) {
    return <span className="markdown-code-path">{path}</span>;
  }
  return (
    <button
      type="button"
      className="markdown-code-path markdown-code-path-link"
      title={filePath}
      onClick={() => onOpenFile(filePath)}
    >
      {path}
    </button>
  );
}

function languageFromFileName(fileName: string): string {
  const lower = fileName.toLowerCase();
  if (lower === "dockerfile") return "dockerfile";
  if (lower === "makefile") return "makefile";
  const ext = lower.includes(".")
    ? lower.slice(lower.lastIndexOf(".") + 1)
    : lower;
  return LANGUAGE_FROM_EXT[ext] ?? ext;
}

function fileNameForLanguage(language: string): string {
  const key = language.toLowerCase();
  return LANGUAGE_FILE_NAMES[key] ?? `code.${key}`;
}

function inlineFileName(value: string): string | undefined {
  const text = value.trim();
  if (!text || text.length > 240 || /\s/.test(text)) return undefined;

  const withoutLocation = text.replace(
    /(?::\d+(?::\d+)?|#L\d+(?:-L\d+)?)$/,
    "",
  );
  const fileName = withoutLocation.split(/[/\\]/).filter(Boolean).pop();
  if (!fileName || !/^[\w@+().-]+$/.test(fileName)) return undefined;

  const lower = fileName.toLowerCase();
  if (
    lower === "dockerfile" ||
    lower === "makefile" ||
    lower === "gemfile" ||
    lower === "license"
  ) {
    return fileName;
  }

  const extension = fileName.split(".").pop();
  return extension && /^[a-z][a-z0-9+-]{0,11}$/i.test(extension)
    ? fileName
    : undefined;
}
