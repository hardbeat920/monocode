import { css } from "@codemirror/lang-css";
import { javascript } from "@codemirror/lang-javascript";
import { markdown } from "@codemirror/lang-markdown";
import { python } from "@codemirror/lang-python";
import { rustLanguage } from "@codemirror/lang-rust";
import { EditorState, type Extension } from "@codemirror/state";
import { describe, expect, it } from "vitest";
import {
  activeOutlineId,
  outlineFromState,
  type OutlineItem,
} from "./editorOutline";

function outline(code: string, path: string, language?: Extension) {
  const state = EditorState.create({
    doc: code,
    extensions: language ? [language] : [],
  });
  return outlineFromState(state, path);
}

function labels(items: OutlineItem[]) {
  return items.map((item) => `${"  ".repeat(item.depth)}${item.label}`);
}

describe("outlineFromState", () => {
  it("reads TypeScript declarations and nests class members", () => {
    const items = outline(
      [
        "interface Foo { a: string }",
        "type Bar = number;",
        "enum E { A, B }",
        "const fn = (a: number) => a + 1;",
        "function hello(name: string) { return name; }",
        "class Widget {",
        "  field = 1;",
        "  method() { const local = 2; return local; }",
        "}",
        "export const topVar = 1;",
      ].join("\n"),
      "/repo/a.ts",
      javascript({ typescript: true }),
    );

    expect(labels(items)).toEqual([
      "Foo",
      "Bar",
      "E",
      "fn",
      "hello",
      "Widget",
      "  field",
      "  method",
      "topVar",
    ]);
    expect(items.map((item) => item.kind)).toEqual([
      "interface",
      "type",
      "enum",
      "variable",
      "function",
      "class",
      "property",
      "method",
      "variable",
    ]);
    expect(items[5].line).toBe(6);
  });

  it("drops function locals but keeps nested definitions", () => {
    const items = outline(
      [
        "def foo(a):",
        "    x = 2",
        "    def inner():",
        "        pass",
        "class Bar:",
        "    def method(self):",
        "        pass",
      ].join("\n"),
      "/repo/a.py",
      python(),
    );

    expect(labels(items)).toEqual(["foo", "  inner", "Bar", "  method"]);
  });

  it("reads Rust items including impl and module nesting", () => {
    const items = outline(
      [
        "const N: i32 = 1;",
        "struct Point { x: i32 }",
        "enum Color { Red }",
        "trait Draw { fn draw(&self); }",
        "impl Point { fn new() -> Self { Point { x: 0 } } }",
        "fn main() { let x = 1; }",
        "mod util { pub fn helper() {} }",
      ].join("\n"),
      "/repo/a.rs",
      rustLanguage,
    );

    expect(labels(items)).toEqual([
      "N",
      "Point",
      "Color",
      "Draw",
      "  draw",
      "impl Point",
      "  new",
      "main",
      "util",
      "  helper",
    ]);
  });

  it("maps Markdown heading levels to depth", () => {
    const items = outline(
      ["# Title", "## Section A", "### Sub", "## Section B"].join("\n"),
      "/repo/a.md",
      markdown(),
    );

    expect(labels(items)).toEqual([
      "Title",
      "  Section A",
      "    Sub",
      "  Section B",
    ]);
    expect(items.every((item) => item.kind === "heading")).toBe(true);
  });

  it("reads CSS selectors, at-rules and nested rules", () => {
    const items = outline(
      [
        "body { color: red; }",
        ".card, .panel { display: flex; }",
        "@media (min-width: 10px) { .nested { color: blue; } }",
        "@keyframes spin { from {} to {} }",
      ].join("\n"),
      "/repo/a.css",
      css(),
    );

    expect(labels(items)).toEqual([
      "body",
      ".card, .panel",
      "@media (min-width: 10px)",
      "  .nested",
      "@keyframes spin",
    ]);
  });

  it("falls back to a line scan for languages without a declaration tree", () => {
    const items = outline(
      ["package main", "func main() {", "}", "type Point struct {", "}"].join(
        "\n",
      ),
      "/repo/main.go",
    );

    expect(labels(items)).toEqual(["main", "Point"]);
    expect(items.map((item) => item.kind)).toEqual(["function", "class"]);
  });

  it("infers depth from indentation in the fallback scan", () => {
    const items = outline(
      ["class Foo", "  def bar", "  end", "end"].join("\n"),
      "/repo/a.rb",
    );

    expect(labels(items)).toEqual(["Foo", "  bar"]);
  });

  it("returns nothing for a plain text file", () => {
    expect(outline("hello\nworld", "/repo/a.txt")).toEqual([]);
  });
});

describe("activeOutlineId", () => {
  const items: OutlineItem[] = [
    { id: "a", label: "a", kind: "class", from: 0, to: 100, line: 1, depth: 0 },
    {
      id: "b",
      label: "b",
      kind: "method",
      from: 20,
      to: 50,
      line: 3,
      depth: 1,
    },
  ];

  it("prefers the innermost item containing the cursor", () => {
    expect(activeOutlineId(items, 30)).toBe("b");
  });

  it("keeps the enclosing item before its first child", () => {
    expect(activeOutlineId(items, 10)).toBe("a");
  });

  it("keeps the last preceding item past the end", () => {
    expect(activeOutlineId(items, 500)).toBe("b");
  });

  it("returns null before any item", () => {
    expect(activeOutlineId([], 0)).toBeNull();
  });
});
