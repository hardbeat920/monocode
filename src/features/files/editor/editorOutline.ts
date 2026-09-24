import { ensureSyntaxTree, syntaxTree } from "@codemirror/language";
import type { EditorState } from "@codemirror/state";
import type { SyntaxNode, Tree } from "@lezer/common";
import { basename } from "../../../platform/tauri/fs";

/**
 * Document outline for the code view.
 *
 * The editor already parses the file for highlighting, so the symbol list is
 * one tree walk over that same tree — no second parser, no language server.
 * Grammars that expose real declarations (JS/TS, Python, Rust, CSS, Markdown)
 * are read structurally; everything else falls back to a line scan so a Go or
 * Ruby file still gets a usable outline.
 */

export type OutlineKind =
  | "function"
  | "method"
  | "class"
  | "interface"
  | "type"
  | "enum"
  | "variable"
  | "constant"
  | "property"
  | "heading"
  | "module"
  | "namespace"
  | "section"
  | "other";

export type OutlineItem = {
  /** Stable per-document key: the declaration's source range. */
  id: string;
  label: string;
  kind: OutlineKind;
  /** Document offset the item starts at, used to reveal it in the editor. */
  from: number;
  to: number;
  /** 1-based line number, for tooltips and tests. */
  line: number;
  /** Nesting level; 0 is top level. */
  depth: number;
};

/** Timebox a full parse so the outline never waits on a huge file's tail. */
const PARSE_BUDGET_MS = 150;

/** Long selectors and `impl ... for ...` clauses stop being readable past this. */
const MAX_LABEL_CHARS = 120;

const NODE_KINDS: Record<string, OutlineKind> = {
  FunctionDeclaration: "function",
  FunctionExpression: "function",
  FunctionItem: "function",
  FunctionDefinition: "function",
  MethodDeclaration: "method",
  MethodDefinition: "method",
  ClassDeclaration: "class",
  ClassExpression: "class",
  ClassDefinition: "class",
  StructItem: "class",
  ImplItem: "class",
  InterfaceDeclaration: "interface",
  TraitItem: "interface",
  TypeAliasDeclaration: "type",
  TypeDefinition: "type",
  TypeItem: "type",
  EnumDeclaration: "enum",
  EnumItem: "enum",
  NamespaceDeclaration: "namespace",
  ModuleDeclaration: "module",
  ModItem: "module",
  PropertyDeclaration: "property",
  PropertySignature: "property",
  FieldDeclaration: "property",
  VariableDeclaration: "variable",
  LexicalDeclaration: "variable",
  ConstItem: "constant",
  StaticItem: "constant",
  RuleSet: "section",
  MediaStatement: "section",
  KeyframesStatement: "section",
};

/** Child node names that carry a declaration's identifier, across grammars. */
const NAME_NODES = new Set([
  "VariableDefinition",
  "VariableName",
  "TypeDefinition",
  "TypeIdentifier",
  "PropertyDefinition",
  "BoundIdentifier",
]);

/** `const fn = () => {}` reads as a function, not a variable. */
const FUNCTION_VALUES = new Set([
  "ArrowFunction",
  "FunctionExpression",
  "Function",
  "ClassExpression",
  "ClassDeclaration",
]);

const HEADING_RE = /^(?:ATX|Setext)Heading([1-6])$/;

export function outlineFromState(
  state: EditorState,
  path: string,
): OutlineItem[] {
  if (state.doc.length === 0) return [];
  const tree =
    ensureSyntaxTree(state, state.doc.length, PARSE_BUDGET_MS) ??
    syntaxTree(state);
  if (tree.length > 0) {
    const items = treeOutline(state, tree);
    if (items.length > 0) return items;
  }
  return fallbackOutline(state, path);
}

function treeOutline(state: EditorState, tree: Tree): OutlineItem[] {
  const items: OutlineItem[] = [];
  walk(state, tree.topNode, 0, items);
  return items;
}

function walk(
  state: EditorState,
  node: SyntaxNode,
  depth: number,
  items: OutlineItem[],
): void {
  const headingLevel = headingDepth(node.name);
  const kind = headingLevel !== null ? "heading" : NODE_KINDS[node.name];
  let childDepth = depth;

  if (kind) {
    const itemDepth = headingLevel !== null ? headingLevel : depth;
    if (isWanted(node, kind, depth)) {
      const label = labelFor(state, node, kind);
      if (label) {
        items.push({
          id: `${node.from}:${node.to}`,
          label,
          kind,
          from: node.from,
          to: node.to,
          line: state.doc.lineAt(node.from).number,
          depth: itemDepth,
        });
        childDepth = depth + 1;
      }
    }
  }

  for (let child = node.firstChild; child; child = child.nextSibling) {
    walk(state, child, childDepth, items);
  }
}

function headingDepth(name: string): number | null {
  const match = HEADING_RE.exec(name);
  if (!match) return null;
  return Number(match[1]) - 1;
}

function isWanted(node: SyntaxNode, kind: OutlineKind, depth: number): boolean {
  // Locals are noise; a `const` is only interesting when it holds a function
  // or class, or when it sits at the top level of the file.
  if (kind === "variable" && depth > 0) return hasFunctionValue(node);
  if (kind === "property" && node.name === "FieldDeclaration") return false;
  return true;
}

function hasFunctionValue(node: SyntaxNode): boolean {
  for (let child = node.firstChild; child; child = child.nextSibling) {
    if (FUNCTION_VALUES.has(child.name)) return true;
  }
  return false;
}

function labelFor(
  state: EditorState,
  node: SyntaxNode,
  kind: OutlineKind,
): string {
  if (kind === "heading") return headingLabel(state, node);
  if (
    node.name === "RuleSet" ||
    node.name === "MediaStatement" ||
    node.name === "KeyframesStatement" ||
    node.name === "ImplItem"
  ) {
    return clip(beforeBrace(state.doc.sliceString(node.from, node.to)));
  }
  if (
    node.name === "VariableDeclaration" ||
    node.name === "LexicalDeclaration"
  ) {
    const names = directChildren(node, "VariableDefinition").map((child) =>
      state.doc.sliceString(child.from, child.to),
    );
    if (names.length > 0) return clip(names.join(", "));
  }
  const name = firstNamedChild(node, NAME_NODES);
  if (name) return clip(state.doc.sliceString(name.from, name.to));
  return "";
}

function headingLabel(state: EditorState, node: SyntaxNode): string {
  const text = state.doc.sliceString(node.from, node.to).split("\n")[0];
  return text
    .replace(/^\s*#{1,6}\s*/, "")
    .replace(/\s*#+\s*$/, "")
    .trim();
}

function beforeBrace(text: string): string {
  return text.split("{")[0].replace(/\s+/g, " ").trim();
}

function clip(label: string): string {
  const flat = label.trim();
  return flat.length > MAX_LABEL_CHARS
    ? `${flat.slice(0, MAX_LABEL_CHARS - 1)}…`
    : flat;
}

function firstNamedChild(
  node: SyntaxNode,
  names: Set<string>,
): SyntaxNode | null {
  for (let child = node.firstChild; child; child = child.nextSibling) {
    if (names.has(child.name)) return child;
  }
  return null;
}

function directChildren(node: SyntaxNode, name: string): SyntaxNode[] {
  const children: SyntaxNode[] = [];
  for (let child = node.firstChild; child; child = child.nextSibling) {
    if (child.name === name) children.push(child);
  }
  return children;
}

/** The declaration nearest the cursor, preferring the innermost enclosing one. */
export function activeOutlineId(
  items: OutlineItem[],
  pos: number,
): string | null {
  let containing: OutlineItem | null = null;
  let preceding: OutlineItem | null = null;
  for (const item of items) {
    if (item.from > pos) continue;
    if (preceding === null || item.from > preceding.from) preceding = item;
    if (
      item.to >= pos &&
      (containing === null || item.from > containing.from)
    ) {
      containing = item;
    }
  }
  return (containing ?? preceding)?.id ?? null;
}

type FallbackPattern = { re: RegExp; kind: OutlineKind };

const CLike: FallbackPattern[] = [
  {
    re: /^\s*(?:(?:public|private|protected|internal|static|final|abstract|open|override|export|async|sealed|partial|virtual|extern|unsafe|inline|constexpr|friend)\s+)*(class|interface|enum|struct|trait|object|protocol|extension|record)\s+([A-Za-z_$][\w$]*)/,
    kind: "class",
  },
  {
    re: /^\s*(?:(?:public|private|protected|internal|static|final|abstract|open|override|export|async|sealed|partial|virtual|extern|unsafe|inline|constexpr)\s+)*(?:func|fn|function)\s+([A-Za-z_$][\w$]*)/,
    kind: "function",
  },
  {
    re: /^\s*(?:(?:public|private|protected|internal|static|final|abstract|open|override|export|async|sealed|partial|virtual|extern|unsafe|inline|constexpr)\s+)*(?:def)\s+([A-Za-z_$][\w$]*)/,
    kind: "method",
  },
  {
    re: /^\s*(?:(?:public|private|protected|internal|static|final|abstract|open|override|export|async|sealed|partial|virtual|extern|unsafe|inline|constexpr)\s+)*(?:[A-Za-z_$][\w$<>\[\],.*&?:.\s]+\s+)?([A-Za-z_$][\w$]*)\s*\([^;{}]*\)\s*(?:const\s*)?(?:throws\s+[\w,\s.]+\s*)?\{/,
    kind: "method",
  },
];

const CLikeNegative = new Set([
  "if",
  "for",
  "while",
  "switch",
  "catch",
  "return",
  "else",
  "do",
  "new",
  "sizeof",
  "typeof",
]);

const FALLBACK_FAMILIES: Record<string, FallbackPattern[]> = {
  go: [
    {
      re: /^\s*func\s+(?:\([^)]*\)\s*)?([A-Za-z_]\w*)\s*\(/,
      kind: "function",
    },
    { re: /^\s*type\s+([A-Za-z_]\w*)\s+(?:struct|interface)\b/, kind: "class" },
  ],
  ruby: [
    { re: /^\s*def\s+(?:self\.)?([A-Za-z_]\w*[!?=]?)/, kind: "method" },
    {
      re: /^\s*(?:class|module)\s+([A-Za-z_]\w*(?:::[A-Za-z_]\w*)*)/,
      kind: "class",
    },
  ],
  shell: [
    {
      re: /^\s*(?:function\s+)?([A-Za-z_][\w-]*)\s*\(\s*\)\s*\{/,
      kind: "function",
    },
  ],
  toml: [{ re: /^\s*\[\[?([^\]]+)\]\]?/, kind: "section" }],
  yaml: [{ re: /^([A-Za-z_][\w.-]*):(?:\s|$)/, kind: "property" }],
  protobuf: [
    { re: /^\s*(?:message|enum|service)\s+([A-Za-z_]\w*)/, kind: "class" },
    { re: /^\s*rpc\s+([A-Za-z_]\w*)/, kind: "method" },
  ],
  lua: [{ re: /^\s*(?:local\s+)?function\s+([\w.:]+)/, kind: "function" }],
  r: [{ re: /^\s*([\w.]+)\s*<-\s*function/, kind: "function" }],
  perl: [{ re: /^\s*sub\s+(\w+)/, kind: "function" }],
  powershell: [{ re: /^\s*function\s+([\w-]+)/, kind: "function" }],
  clike: CLike,
};

const FAMILY_BY_EXTENSION: Record<string, string> = {
  ".go": "go",
  ".rb": "ruby",
  ".rake": "ruby",
  ".sh": "shell",
  ".bash": "shell",
  ".zsh": "shell",
  ".toml": "toml",
  ".yaml": "yaml",
  ".yml": "yaml",
  ".proto": "protobuf",
  ".lua": "lua",
  ".r": "r",
  ".pl": "perl",
  ".pm": "perl",
  ".ps1": "powershell",
  ".psd1": "powershell",
  ".psm1": "powershell",
  ".c": "clike",
  ".h": "clike",
  ".cc": "clike",
  ".cpp": "clike",
  ".cxx": "clike",
  ".hh": "clike",
  ".hpp": "clike",
  ".hxx": "clike",
  ".cs": "clike",
  ".java": "clike",
  ".php": "clike",
  ".phtml": "clike",
  ".dart": "clike",
  ".swift": "clike",
  ".kt": "clike",
  ".kts": "clike",
  ".scala": "clike",
  ".sc": "clike",
  ".m": "clike",
  ".mm": "clike",
  ".sql": "clike",
};

function fallbackOutline(state: EditorState, path: string): OutlineItem[] {
  const name = basename(path).toLowerCase();
  const extension = name.includes(".") ? name.slice(name.lastIndexOf(".")) : "";
  const family = FAMILY_BY_EXTENSION[extension];
  if (!family) return [];
  const patterns = FALLBACK_FAMILIES[family];
  if (!patterns) return [];

  const items: OutlineItem[] = [];
  const indentStack: number[] = [];
  const lines = state.doc;

  for (let lineNumber = 1; lineNumber <= lines.lines; lineNumber++) {
    const line = lines.line(lineNumber);
    for (const pattern of patterns) {
      const match = pattern.re.exec(line.text);
      if (!match) continue;
      const label = match[2] ?? match[1];
      if (!label || CLikeNegative.has(label)) continue;
      const indent = leadingIndent(line.text);
      while (
        indentStack.length > 0 &&
        indentStack[indentStack.length - 1] >= indent
      ) {
        indentStack.pop();
      }
      const depth = indentStack.length;
      indentStack.push(indent);
      items.push({
        id: `${line.from}:${line.to}`,
        label: clip(label),
        kind: pattern.kind,
        from: line.from,
        to: line.to,
        line: lineNumber,
        depth,
      });
      break;
    }
  }
  return items;
}

function leadingIndent(text: string): number {
  let columns = 0;
  for (const character of text) {
    if (character === " ") columns += 1;
    else if (character === "\t") columns += 4;
    else break;
  }
  return columns;
}
