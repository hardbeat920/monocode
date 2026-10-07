import { describe, expect, it } from "vitest";
import { definitionFor, symbolAt } from "./editorSymbolNavigation";

describe("editor symbol navigation", () => {
  it("finds the identifier at either side of a cursor", () => {
    expect(symbolAt("callThing(value)", 5)).toEqual({
      name: "callThing",
      from: 0,
      to: 9,
    });
    expect(symbolAt("callThing(value)", 9)).toMatchObject({
      name: "callThing",
    });
    expect(symbolAt("()", 0)).toBeNull();
  });

  it("prefers a function declaration over references", () => {
    const definition = definitionFor("renderItem", [
      {
        path: "/repo/a.ts",
        relative: "a.ts",
        line: 4,
        column: 1,
        preview: "renderItem(value);",
      },
      {
        path: "/repo/b.ts",
        relative: "b.ts",
        line: 12,
        column: 17,
        preview: "export function renderItem(value: string) {",
      },
    ]);
    expect(definition).toMatchObject({ path: "/repo/b.ts", line: 12 });
  });

  it("does not match an identifier prefix that ends in a dollar sign", () => {
    const definition = definitionFor("foo", [
      {
        path: "/repo/a.ts",
        relative: "a.ts",
        line: 1,
        column: 1,
        preview: "function foo$() {}",
      },
      {
        path: "/repo/b.ts",
        relative: "b.ts",
        line: 2,
        column: 1,
        preview: "function foo() {}",
      },
    ]);
    expect(definition).toMatchObject({ path: "/repo/b.ts", line: 2 });
  });

  it("does not treat commented declarations as definitions", () => {
    const definition = definitionFor("foo", [
      {
        path: "/repo/a.ts",
        relative: "a.ts",
        line: 1,
        column: 4,
        preview: "// function foo() {}",
      },
      {
        path: "/repo/b.ts",
        relative: "b.ts",
        line: 2,
        column: 1,
        preview: "function foo() {}",
      },
    ]);
    expect(definition).toMatchObject({ path: "/repo/b.ts", line: 2 });
  });
});
