import { closeBrackets, closeBracketsKeymap } from "@codemirror/autocomplete";
import {
  bracketMatching,
  getIndentUnit,
  indentOnInput,
  indentUnit,
  syntaxTree,
} from "@codemirror/language";
import {
  Facet,
  Prec,
  StateField,
  type EditorState,
  type Extension,
} from "@codemirror/state";
import {
  Decoration,
  EditorView,
  keymap,
  type DecorationSet,
} from "@codemirror/view";
import type { SyntaxNode } from "@lezer/common";
import { basename } from "../../../platform/tauri/fs";

export const editorMatching: Extension = [bracketMatching(), matchingTags()];

export function editorTyping(path: string): Extension {
  const syntax = syntaxForPath(path);
  return [
    closeBrackets(),
    indentOnInput(),
    Prec.high(keymap.of(closeBracketsKeymap)),
    syntax ? emmetSyntax.of(syntax) : [],
  ];
}

export function tryExpandEmmet(view: EditorView): boolean {
  const syntax = view.state.facet(emmetSyntax);
  if (!syntax || view.state.readOnly) return false;

  const range = view.state.selection.main;
  if (!range.empty || view.state.selection.ranges.length !== 1) return false;

  const line = view.state.doc.lineAt(range.head);
  const column = range.head - line.from;
  if (column < line.text.length && /\w/.test(line.text[column] ?? "")) {
    return false;
  }

  const abbr = abbreviationBefore(line.text, column);
  if (!abbr || !isEmmetAbbr(abbr, syntax)) return false;

  const from = range.head - abbr.length;
  if (!canExpandAt(view.state, from)) return false;

  const pieces = parseAbbr(abbr);
  if (!pieces) return false;

  const indent = line.text.match(/^\s*/)?.[0] ?? "";
  const unit = view.state.facet(indentUnit) || " ".repeat(getIndentUnit(view.state));
  const expanded = expandPieces(pieces, syntax === "jsx", indent, unit);
  if (!expanded.text || expanded.text === abbr) return false;

  view.dispatch({
    changes: { from, to: range.head, insert: expanded.text },
    selection: { anchor: from + expanded.cursor },
    userEvent: "input.complete",
  });
  return true;
}

type EmmetSyntax = "html" | "jsx";

const emmetSyntax = Facet.define<EmmetSyntax, EmmetSyntax | null>({
  combine: (values) => values[values.length - 1] ?? null,
});

function syntaxForPath(path: string): EmmetSyntax | null {
  const name = basename(path).toLowerCase();
  const extension = name.includes(".") ? name.slice(name.lastIndexOf(".")) : "";
  if ([".html", ".htm", ".md", ".mdx", ".markdown"].includes(extension)) {
    return "html";
  }
  if ([".jsx", ".tsx"].includes(extension)) return "jsx";
  return null;
}

const HTML_TAGS = new Set(
  "a abbr address area article aside audio b base bdi bdo blockquote body br button canvas caption cite code col colgroup data datalist dd del details dfn dialog div dl dt em embed fieldset figcaption figure footer form h1 h2 h3 h4 h5 h6 head header hgroup hr html i iframe img input ins kbd label legend li link main map mark menu meta meter nav noscript object ol optgroup option output p picture pre progress q rp rt ruby s samp script search section select slot small source span strong style sub summary sup svg path g circle rect line polyline polygon ellipse text tspan defs table tbody td template textarea tfoot th thead time title tr track u ul var video wbr".split(
    " ",
  ),
);

const SELF_CLOSING = new Set(
  "area base br col embed hr img input link meta param source track wbr".split(
    " ",
  ),
);

const ALWAYS_FORBIDDEN = new Set([
  "ArrayExpression",
  "AssignmentExpression",
  "Attribute",
  "AttributeValue",
  "BlockComment",
  "Comment",
  "ImportDeclaration",
  "JSXAttribute",
  "JSXAttributeValue",
  "JSXCloseTag",
  "JSXEscape",
  "JSXOpenTag",
  "JSXSelfClosingTag",
  "LineComment",
  "MemberExpression",
  "ObjectExpression",
  "OpenTag",
  "Property",
  "PropertyDefinition",
  "PropertyName",
  "String",
  "TagName",
  "TemplateString",
  "TypeAnnotation",
  "VariableDefinition",
]);

const FORBIDDEN_UNLESS_LINE_START = new Set([
  "ArgList",
  "CallExpression",
  "ParamList",
]);

type Piece = {
  tag: string;
  id?: string;
  classes: string[];
  count: number;
};

function abbreviationBefore(text: string, column: number): string | null {
  const before = text.slice(0, column);
  let start = before.length;
  while (start > 0 && !/[\s=;,{()"'`]/.test(before[start - 1] ?? "")) {
    start -= 1;
  }
  const abbr = before.slice(start);
  return abbr || null;
}

function isEmmetAbbr(abbr: string, syntax: EmmetSyntax): boolean {
  if (abbr.startsWith(".") || abbr.startsWith("#")) return true;
  const tag = abbr.split(/[.#*]/, 1)[0]?.split(">", 1)[0] ?? "";
  if (!tag) return false;
  if (syntax === "jsx" && /^[A-Z][\w]*$/.test(tag)) return true;
  return HTML_TAGS.has(tag.toLowerCase());
}

function canExpandAt(state: EditorState, pos: number): boolean {
  const line = state.doc.lineAt(pos);
  const prefix = line.text.slice(0, pos - line.from);
  if (
    /\b(const|let|var|function|class|import|export|typeof|new|await|void|yield|case|throw|else|of|in|as|from)\s+$/.test(
      prefix,
    )
  ) {
    return false;
  }

  const atLineStart = /^\s*$/.test(prefix);
  for (
    let node: SyntaxNode | null = syntaxTree(state).resolveInner(pos, -1);
    node;
    node = node.parent
  ) {
    if (ALWAYS_FORBIDDEN.has(node.name)) return false;
    if (!atLineStart && FORBIDDEN_UNLESS_LINE_START.has(node.name)) {
      return false;
    }
  }
  return true;
}

function parseAbbr(abbr: string): Piece[] | null {
  const parts = abbr.split(">").map((part) => part.trim());
  if (!parts.length || parts.some((part) => !part)) return null;
  const pieces = [];
  for (const part of parts) {
    const piece = parsePiece(part);
    if (!piece) return null;
    pieces.push(piece);
  }
  return pieces;
}

function parsePiece(part: string): Piece | null {
  let rest = part;
  let count = 1;
  const star = rest.match(/\*(\d+)$/);
  if (star?.index != null) {
    count = Number(star[1]);
    if (!count) return null;
    rest = rest.slice(0, star.index);
  }

  let tag = "";
  let id: string | undefined;
  const classes: string[] = [];
  while (rest) {
    if (rest.startsWith("#")) {
      const match = rest.match(/^#([\w-]+)/);
      if (!match) return null;
      id = match[1];
      rest = rest.slice(match[0].length);
      continue;
    }
    if (rest.startsWith(".")) {
      const match = rest.match(/^\.([\w-]+)/);
      if (!match) return null;
      classes.push(match[1]);
      rest = rest.slice(match[0].length);
      continue;
    }
    const match = rest.match(/^([A-Za-z][\w:-]*)/);
    if (!match || tag) return null;
    tag = match[1];
    rest = rest.slice(match[0].length);
  }
  if (!tag) {
    if (!id && classes.length === 0) return null;
    tag = "div";
  }
  return { tag, id, classes, count };
}

function expandPieces(
  pieces: Piece[],
  jsx: boolean,
  indent: string,
  unit: string,
): { text: string; cursor: number } {
  let text = "";
  let cursor = 0;
  let placed = false;

  const write = (chunk: string) => {
    text += chunk;
  };

  const render = (index: number, level: number) => {
    const piece = pieces[index];
    if (!piece) return;
    const pad = indent + unit.repeat(level);
    const last = index === pieces.length - 1;
    const voidTag = last && SELF_CLOSING.has(piece.tag.toLowerCase());
    const attrs = pieceAttributes(piece, jsx);

    for (let i = 0; i < piece.count; i += 1) {
      if (i > 0) write(`\n${pad}`);
      if (voidTag) {
        write(`<${piece.tag}${attrs} />`);
        if (!placed) {
          cursor = text.length;
          placed = true;
        }
        continue;
      }
      write(`<${piece.tag}${attrs}>`);
      if (last) {
        if (!placed) {
          cursor = text.length;
          placed = true;
        }
        write(`</${piece.tag}>`);
      } else {
        write(`\n${indent}${unit.repeat(level + 1)}`);
        render(index + 1, level + 1);
        write(`\n${pad}</${piece.tag}>`);
      }
    }
  };

  render(0, 0);
  return { text, cursor };
}

function pieceAttributes(piece: Piece, jsx: boolean): string {
  const attrs = [];
  if (piece.id) attrs.push(`id="${piece.id}"`);
  if (piece.classes.length) {
    const name = jsx ? "className" : "class";
    attrs.push(`${name}="${piece.classes.join(" ")}"`);
  }
  return attrs.length ? ` ${attrs.join(" ")}` : "";
}

const TAG_NODES = new Set(["OpenTag", "CloseTag", "JSXOpenTag", "JSXCloseTag"]);
const ELEMENT_NODES = new Set(["Element", "JSXElement"]);
const NAME_NODES = new Set([
  "TagName",
  "JSXIdentifier",
  "JSXBuiltin",
  "JSXMemberExpression",
  "JSXNamespacedName",
]);

const matchingMark = Decoration.mark({ class: "cm-matchingBracket" });

function matchingTags(): Extension {
  return StateField.define<DecorationSet>({
    create: (state) => tagDecorations(state),
    update(value, transaction) {
      if (!transaction.docChanged && !transaction.selection) return value;
      return tagDecorations(transaction.state);
    },
    provide: (field) => EditorView.decorations.from(field),
  });
}

function tagDecorations(state: EditorState): DecorationSet {
  const ranges = [];
  for (const range of state.selection.ranges) {
    if (!range.empty) continue;
    const pair = tagPairAt(state, range.head);
    if (!pair) continue;
    ranges.push(matchingMark.range(pair.open.from, pair.open.to));
    ranges.push(matchingMark.range(pair.close.from, pair.close.to));
  }
  return Decoration.set(ranges, true);
}

function tagPairAt(state: EditorState, pos: number) {
  const tree = syntaxTree(state);
  const tag =
    tagFromCursor(tree.resolveInner(pos, 1)) ??
    tagFromCursor(tree.resolveInner(pos, -1));
  if (!tag?.parent || !ELEMENT_NODES.has(tag.parent.name)) return null;

  const open = tag.parent.firstChild;
  const close = tag.parent.lastChild;
  if (!open || !close || open === close) return null;
  if (!TAG_NODES.has(open.name) || !TAG_NODES.has(close.name)) return null;

  const openName = nameChild(open);
  const closeName = nameChild(close);
  if (!openName || !closeName) return null;
  return { open: openName, close: closeName };
}

function tagFromCursor(node: SyntaxNode | null): SyntaxNode | null {
  for (let current = node; current; current = current.parent) {
    if (TAG_NODES.has(current.name)) return current;
    if (ELEMENT_NODES.has(current.name)) return null;
  }
  return null;
}

function nameChild(tag: SyntaxNode): SyntaxNode | null {
  for (let child = tag.firstChild; child; child = child.nextSibling) {
    if (NAME_NODES.has(child.name)) return child;
  }
  return null;
}
