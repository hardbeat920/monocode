import { javascript } from "@codemirror/lang-javascript";
import { describe, expect, it } from "vitest";
import { buildUnifiedFile } from "../../source-control/model/unifiedDiff";
import { languageForPath } from "./editorLanguage";
import { highlightDiffFile, highlightSource } from "./syntaxTokens";

const KEYWORD = "var(--syntax-keyword)";
const STRING = "var(--syntax-string)";
const COMMENT = "var(--syntax-comment)";
const PROPERTY = "var(--syntax-property)";

describe("highlightSource", () => {
  it("colors script blocks in Vue and Svelte components", async () => {
    for (const path of ["App.vue", "Counter.svelte"]) {
      const language = await languageForPath(path);
      const lines = highlightSource(
        '<script>\nconst name = "agent";\n</script>',
        language,
      );
      expect(token(lines[1], "const")?.color).toBe(KEYWORD);
      expect(token(lines[1], '"agent"')?.color).toBe(STRING);
    }
  });

  it.each(["theme.scss", "theme.less"])(
    "colors comments in %s stylesheets",
    async (path) => {
      const language = await languageForPath(path);
      const lines = highlightSource("// note\na { color: red; }", language);
      expect(token(lines[0], "// note")?.color).toBe(COMMENT);
    },
  );

  it("colors declarations in indented Sass stylesheets", async () => {
    const language = await languageForPath("theme.sass");
    const lines = highlightSource(".card\n  color: red", language);
    expect(token(lines[1], "color")?.color).toBe(PROPERTY);
  });

  it("colors TypeScript keywords, strings, and comments", () => {
    const lines = highlightSource(
      'const name = "agent";\n// note',
      javascript({ typescript: true }),
    );
    expect(token(lines[0], "const")?.color).toBe(KEYWORD);
    expect(token(lines[0], '"agent"')?.color).toBe(STRING);
    expect(token(lines[1], "// note")?.color).toBe(COMMENT);
  });

  it("colors comments in JSONC files", async () => {
    const language = await languageForPath("settings.jsonc");
    const lines = highlightSource('{\n  // note\n  "a": 1\n}', language);
    expect(token(lines[1], "// note")?.color).toBe(COMMENT);
  });

  it("colors dotenv keys and leaves values as plain text", async () => {
    const language = await languageForPath(".env");
    const lines = highlightSource("# note\nAWS_BUCKET=bucket", language);
    expect(token(lines[0], "# note")?.color).toBe(COMMENT);
    expect(token(lines[1], "AWS_BUCKET")?.color).toBe("var(--syntax-callable)");
    expect(token(lines[1], "bucket")?.color).toBeUndefined();
  });

  it("leaves unknown languages unstyled", () => {
    const lines = highlightSource("plain text", null);
    expect(lines).toEqual([[{ text: "plain text" }]]);
  });
});

describe("highlightDiffFile", () => {
  it("highlights added and deleted lines from each side", async () => {
    const diff = buildUnifiedFile("const alpha = 1;\n", "const beta = 1;\n");
    const tokens = await highlightDiffFile({
      path: "src/lib/settings.ts",
      blocks: diff.blocks,
    });
    const deleted = diff.lines.find((line) => line.kind === "del");
    const added = diff.lines.find((line) => line.kind === "add");
    expect(deleted && token(tokens.get(deleted), "const")?.color).toBe(KEYWORD);
    expect(added && token(tokens.get(added), "const")?.color).toBe(KEYWORD);
  });

  it("skips decorative parsing for a very large diff", async () => {
    const line = {
      kind: "add" as const,
      text: "x".repeat(250_001),
      oldNumber: null,
      newNumber: 1,
    };
    const tokens = await highlightDiffFile({
      path: "large.ts",
      blocks: [{ kind: "hunk", lines: [line] }],
    });
    expect(tokens.size).toBe(0);
  });
});

function token(
  line: { text: string; color?: string }[] | undefined,
  text: string,
) {
  return line?.find(
    (piece) => piece.text.includes(text) || piece.text === text,
  );
}
