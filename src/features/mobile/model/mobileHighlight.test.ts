import { describe, expect, it } from "vitest";

import { highlightDiffForMobile, highlightForMobile, languageForFile } from "./mobileHighlight";

describe("languageForFile", () => {
  it("knows languages by extension, alias and file name", () => {
    expect(languageForFile("src/app.ts")).toBe("typescript");
    expect(languageForFile("src/App.tsx")).toBe("tsx");
    expect(languageForFile("scripts/build.mjs")).toBe("javascript");
    expect(languageForFile("Cargo.toml")).toBe("toml");
    expect(languageForFile("docker/Dockerfile")).toBe("docker");
    expect(languageForFile("README.md")).toBe("markdown");
  });

  it("leaves unknown files plain", () => {
    expect(languageForFile("LICENSE")).toBeUndefined();
    expect(languageForFile("image.png")).toBeUndefined();
  });
});

describe("highlightForMobile", () => {
  it("returns every line as coloured runs that rebuild the text", async () => {
    const text = 'const greeting = "hi";\n\nexport default greeting;';
    const result = await highlightForMobile("greeting.ts", text);
    expect(result).toBeDefined();
    const { palette, lines } = result!;
    expect(lines.map((line) => line.map(([content]) => content).join(""))).toEqual(
      text.split("\n"),
    );
    expect(palette.length).toBeGreaterThan(1);
    for (const line of lines) {
      for (const [, color] of line) expect(palette[color]).toMatch(/^#/);
    }
  });

  it("skips files it can't or shouldn't highlight", async () => {
    expect(await highlightForMobile("notes", "plain")).toBeUndefined();
    expect(await highlightForMobile("big.ts", "x".repeat(200_000))).toBeUndefined();
  });
});

describe("highlightDiffForMobile", () => {
  it("colours each line from its own side of the diff", async () => {
    const lines = [
      { kind: "hunk" as const, text: "@@ -1,2 +1,2 @@", oldNumber: null, newNumber: null },
      { kind: "context" as const, text: "const a = 1;", oldNumber: 1, newNumber: 1 },
      { kind: "del" as const, text: 'const b = "old";', oldNumber: 2, newNumber: null },
      { kind: "add" as const, text: "const b = 2;", oldNumber: null, newNumber: 2 },
    ];
    const result = await highlightDiffForMobile(
      "file.ts",
      'const a = 1;\nconst b = "old";',
      "const a = 1;\nconst b = 2;",
      lines,
    );
    expect(result).toBeDefined();
    const { palette, lines: coloured } = result!;
    expect(coloured[0].runs).toBeUndefined();
    for (const line of coloured.slice(1)) {
      expect(line.runs?.map(([text]) => text).join("")).toBe(line.text);
      for (const [, color] of line.runs!) expect(palette[color]).toMatch(/^#/);
    }
  });
});
