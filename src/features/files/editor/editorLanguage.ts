import {
  HighlightStyle,
  LanguageSupport,
  StreamLanguage,
  type StreamParser,
} from "@codemirror/language";
import type { Extension } from "@codemirror/state";
import {
  tagHighlighter,
  tags,
  type Highlighter,
  type Tag,
} from "@lezer/highlight";
import { basename } from "../../../platform/tauri/fs";

const HIGHLIGHT_TAGS = {
  keyword: [
    tags.keyword,
    tags.controlKeyword,
    tags.definitionKeyword,
    tags.moduleKeyword,
    tags.operatorKeyword,
    tags.modifier,
    tags.self,
    tags.bool,
    tags.null,
    tags.atom,
    tags.unit,
  ],
  callable: [
    tags.function(tags.variableName),
    tags.function(tags.propertyName),
    tags.labelName,
    tags.macroName,
  ],
  string: [
    tags.string,
    tags.docString,
    tags.character,
    tags.attributeValue,
    tags.special(tags.string),
    tags.regexp,
    tags.escape,
  ],
  type: [
    tags.typeName,
    tags.className,
    tags.namespace,
    tags.standard(tags.typeName),
  ],
  tag: [tags.tagName],
  number: [tags.number, tags.integer, tags.float],
  comment: [tags.comment, tags.lineComment, tags.blockComment, tags.docComment],
  property: [tags.propertyName, tags.attributeName],
  meta: [tags.meta, tags.processingInstruction, tags.annotation],
  heading: [
    tags.heading,
    tags.heading1,
    tags.heading2,
    tags.heading3,
    tags.heading4,
    tags.heading5,
    tags.heading6,
  ],
};

type HighlightPalette = Record<keyof typeof HIGHLIGHT_TAGS | "invalid", string>;

// Every colour is a `--syntax-*` variable (index.css), so a scheme flip or a
// workspace theme recolours code without highlighting it again.
const HIGHLIGHT_PALETTE: HighlightPalette = {
  keyword: "var(--syntax-keyword)",
  heading: "var(--color-markdown-heading)",
  callable: "var(--syntax-callable)",
  string: "var(--syntax-string)",
  type: "var(--syntax-type)",
  tag: "var(--syntax-tag)",
  number: "var(--syntax-number)",
  comment: "var(--syntax-comment)",
  property: "var(--syntax-property)",
  meta: "var(--syntax-meta)",
  invalid: "var(--syntax-invalid)",
};

const HIGHLIGHT_GROUPS = Object.entries(HIGHLIGHT_TAGS) as [
  keyof typeof HIGHLIGHT_TAGS,
  Tag[],
][];

export const editorHighlightStyle = HighlightStyle.define([
  ...HIGHLIGHT_GROUPS.map(([group, tag]) => ({
    tag,
    color: HIGHLIGHT_PALETTE[group],
  })),
  {
    tag: tags.invalid,
    color: HIGHLIGHT_PALETTE.invalid,
    textDecoration: "underline",
  },
]);

/** Same tag → color map as the editor, for highlighting outside CodeMirror. */
export const syntaxTagHighlighter: Highlighter = tagHighlighter([
  ...HIGHLIGHT_GROUPS.map(([group, tag]) => ({
    tag,
    class: HIGHLIGHT_PALETTE[group],
  })),
  { tag: tags.invalid, class: HIGHLIGHT_PALETTE.invalid },
]);

function legacyLanguage(parser: StreamParser<unknown>): Extension {
  return StreamLanguage.define(parser);
}

export async function languageForPath(path: string): Promise<Extension | null> {
  const name = basename(path).toLowerCase();
  const extension = name.includes(".") ? name.slice(name.lastIndexOf(".")) : "";

  if ([".js", ".jsx", ".mjs", ".cjs", ".ts", ".tsx"].includes(extension)) {
    const { javascript } = await import("@codemirror/lang-javascript");
    return javascript({
      jsx: extension === ".jsx" || extension === ".tsx",
      typescript: extension === ".ts" || extension === ".tsx",
    });
  }
  if (extension === ".json" || name === "package-lock.json") {
    const { json } = await import("@codemirror/lang-json");
    return json();
  }
  if (extension === ".jsonc") {
    // Lezer's JSON grammar rejects comments; the legacy JS-based mode tokenizes them.
    const { json } = await import("@codemirror/legacy-modes/mode/javascript");
    return legacyLanguage(json);
  }
  if (extension === ".css") {
    const { css } = await import("@codemirror/lang-css");
    return css();
  }
  if ([".scss", ".sass"].includes(extension)) {
    const { sass } = await import("@codemirror/lang-sass");
    return sass({ indented: extension === ".sass" });
  }
  if (extension === ".less") {
    const { less } = await import("@codemirror/lang-less");
    return less();
  }
  if ([".html", ".htm"].includes(extension)) {
    const { html } = await import("@codemirror/lang-html");
    return html();
  }
  if (extension === ".vue") {
    const { vue } = await import("@codemirror/lang-vue");
    return vue();
  }
  if (extension === ".svelte") {
    const { svelte } = await import("@replit/codemirror-lang-svelte");
    return svelte();
  }
  if ([".md", ".mdx", ".markdown"].includes(extension)) {
    const { markdown } = await import("@codemirror/lang-markdown");
    return markdown();
  }
  if (extension === ".rs") {
    const { rustLanguage } = await import("@codemirror/lang-rust");
    const { completeFromList } = await import("@codemirror/autocomplete");
    const keywords =
      "as async await break const continue crate dyn else enum extern false fn for if impl in let loop match mod move mut pub ref return self Self static struct super trait true type unsafe use where while"
        .split(" ")
        .map((label) => ({ label, type: "keyword" }));
    return new LanguageSupport(rustLanguage, [
      rustLanguage.data.of({ autocomplete: completeFromList(keywords) }),
    ]);
  }
  if (extension === ".py") {
    const { python } = await import("@codemirror/lang-python");
    return python();
  }
  if (extension === ".c") {
    const { c } = await import("@codemirror/legacy-modes/mode/clike");
    return legacyLanguage(c);
  }
  if (
    [".h", ".cc", ".cpp", ".cxx", ".hh", ".hpp", ".hxx"].includes(extension)
  ) {
    const { cpp } = await import("@codemirror/lang-cpp");
    return cpp();
  }
  if (extension === ".java") {
    const { java } = await import("@codemirror/lang-java");
    return java();
  }
  if ([".php", ".phtml"].includes(extension)) {
    const { php } = await import("@codemirror/lang-php");
    return php();
  }
  if (extension === ".sql") {
    const { sql } = await import("@codemirror/lang-sql");
    return sql();
  }
  if ([".xml", ".svg"].includes(extension)) {
    const { xml } = await import("@codemirror/lang-xml");
    return xml();
  }
  if ([".yaml", ".yml"].includes(extension)) {
    const { yaml } = await import("@codemirror/lang-yaml");
    return yaml();
  }
  if (extension === ".cs") {
    const { csharp } = await import("@codemirror/legacy-modes/mode/clike");
    return legacyLanguage(csharp);
  }
  if (extension === ".go") {
    const { go } = await import("@codemirror/legacy-modes/mode/go");
    return legacyLanguage(go);
  }
  if (extension === ".dart") {
    const { dart } = await import("@codemirror/legacy-modes/mode/clike");
    return legacyLanguage(dart);
  }
  if (extension === ".swift") {
    const { swift } = await import("@codemirror/legacy-modes/mode/swift");
    return legacyLanguage(swift);
  }
  if ([".kt", ".kts"].includes(extension)) {
    const { kotlin } = await import("@codemirror/legacy-modes/mode/clike");
    return legacyLanguage(kotlin);
  }
  if (
    [".rb", ".rake"].includes(extension) ||
    ["gemfile", "rakefile"].includes(name)
  ) {
    const { ruby } = await import("@codemirror/legacy-modes/mode/ruby");
    return legacyLanguage(ruby);
  }
  if (
    [".sh", ".bash", ".zsh"].includes(extension) ||
    [".bashrc", ".bash_profile", ".zshrc", ".zprofile"].includes(name)
  ) {
    const { shell } = await import("@codemirror/legacy-modes/mode/shell");
    return legacyLanguage(shell);
  }
  if (name === ".env" || name.startsWith(".env.")) {
    const { properties } =
      await import("@codemirror/legacy-modes/mode/properties");
    // Keys come out as "def", which the palette leaves uncolored and the
    // built-in token table won't let us remap; rename them. Values stay plain.
    return legacyLanguage({
      ...properties,
      token: (stream, state) => {
        const style = properties.token(stream, state);
        return style === "def" ? "envKey" : style;
      },
      tokenTable: { envKey: tags.labelName },
    });
  }
  if (extension === ".toml") {
    const { toml } = await import("@codemirror/legacy-modes/mode/toml");
    return legacyLanguage(toml);
  }
  if ([".scala", ".sc"].includes(extension)) {
    const { scala } = await import("@codemirror/legacy-modes/mode/clike");
    return legacyLanguage(scala);
  }
  if (extension === ".lua") {
    const { lua } = await import("@codemirror/legacy-modes/mode/lua");
    return legacyLanguage(lua);
  }
  if (extension === ".r") {
    const { r } = await import("@codemirror/legacy-modes/mode/r");
    return legacyLanguage(r);
  }
  if ([".pl", ".pm"].includes(extension)) {
    const { perl } = await import("@codemirror/legacy-modes/mode/perl");
    return legacyLanguage(perl);
  }
  if ([".ps1", ".psd1", ".psm1"].includes(extension)) {
    const { powerShell } =
      await import("@codemirror/legacy-modes/mode/powershell");
    return legacyLanguage(powerShell);
  }
  if ([".m", ".mm"].includes(extension)) {
    const { objectiveC, objectiveCpp } =
      await import("@codemirror/legacy-modes/mode/clike");
    return legacyLanguage(extension === ".mm" ? objectiveCpp : objectiveC);
  }
  if (extension === ".proto") {
    const { protobuf } = await import("@codemirror/legacy-modes/mode/protobuf");
    return legacyLanguage(protobuf);
  }
  if (name === "dockerfile" || name.startsWith("dockerfile.")) {
    const { dockerFile } =
      await import("@codemirror/legacy-modes/mode/dockerfile");
    return legacyLanguage(dockerFile);
  }
  return null;
}
