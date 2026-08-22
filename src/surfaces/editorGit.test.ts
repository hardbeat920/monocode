import { Text } from "@codemirror/state";
import { describe, expect, it } from "vitest";
import { Chunk } from "@codemirror/merge";
import {
  findChunk,
  revertChunkText,
  stageChunkText,
  deletedLineTexts,
  overviewTicks,
  stateWithGitOriginal,
  diffChunks,
} from "./editorGit";

describe("stageChunkText", () => {
  it("stages a whole added hunk and leaves later hunks unstaged", () => {
    const original = "alpha\nbeta\ngamma\ndelta\n";
    const current = "alpha\nBETA\ngamma\nDELTA\n";
    const beta = Text.of(current.split("\n")).line(2).from;
    expect(stageChunkText(original, current, beta)).toBe(
      "alpha\nBETA\ngamma\ndelta\n",
    );
  });

  it("stages selected added lines and excludes the rest of the hunk", () => {
    const original = "alpha\ngamma\n";
    const current = "alpha\nbeta\ndelta\ngamma\n";
    const doc = Text.of(current.split("\n"));
    const beta = doc.line(2);
    expect(
      stageChunkText(original, current, beta.from, {
        from: beta.from,
        to: beta.to,
      }),
    ).toBe("alpha\nbeta\ngamma\n");
  });

  it("stages a deleted hunk", () => {
    const original = "alpha\nbeta\ngamma\n";
    const current = "alpha\ngamma\n";
    const pos = Text.of(current.split("\n")).line(2).from;
    expect(stageChunkText(original, current, pos)).toBe(current);
  });
});

describe("revertChunkText", () => {
  it("restores a deleted line", () => {
    const original = "alpha\nbeta\ngamma\n";
    const current = "alpha\ngamma\n";
    const reverted = revertChunkText(original, current, 6);
    expect(reverted).toBe(original);
  });

  it("removes an added line", () => {
    const original = "alpha\ngamma\n";
    const current = "alpha\nbeta\ngamma\n";
    const reverted = revertChunkText(original, current, 6);
    expect(reverted).toBe(original);
  });

  it("restores a modified hunk", () => {
    const original = "alpha\nbeta\ngamma\n";
    const current = "alpha\nBETA\ngamma\n";
    const reverted = revertChunkText(original, current, 6);
    expect(reverted).toBe(original);
  });

  it("returns null when the cursor is on an unchanged line", () => {
    const original = "alpha\nbeta\ngamma\n";
    const current = "alpha\nBETA\ngamma\n";
    expect(revertChunkText(original, current, 0)).toBeNull();
  });

  it("reverts selected added lines and keeps the rest of the hunk", () => {
    const original = "alpha\ngamma\n";
    const current = "alpha\nbeta\ndelta\ngamma\n";
    const doc = Text.of(current.split("\n"));
    const beta = doc.line(2);
    expect(
      revertChunkText(original, current, beta.from, {
        from: beta.from,
        to: beta.to,
      }),
    ).toBe("alpha\ndelta\ngamma\n");
  });
});

describe("deletedLineTexts", () => {
  it("returns the removed lines for a deletion hunk", () => {
    const original = Text.of("alpha\nbeta\ngamma\n".split("\n"));
    const current = Text.of("alpha\ngamma\n".split("\n"));
    const chunks = Chunk.build(original, current, {
      scanLimit: 5_000,
      timeout: 100,
    });
    const chunk = chunks.find((entry) => entry.fromA !== entry.toA);
    expect(chunk).toBeTruthy();
    expect(deletedLineTexts(original, chunk!)).toEqual(["beta"]);
  });
});

describe("overviewTicks", () => {
  it("maps added, deleted, and modified hunks", () => {
    const added = Chunk.build(
      Text.of("alpha\ngamma\n".split("\n")),
      Text.of("alpha\nbeta\ngamma\n".split("\n")),
    );
    expect(overviewTicks(Text.of("alpha\nbeta\ngamma\n".split("\n")), added, null)).toEqual([
      { kind: "add", top: 1 / 4, size: 1 / 4, pos: 6 },
    ]);

    const original = Text.of("alpha\nbeta\ngamma\n".split("\n"));
    const deletedDoc = Text.of("alpha\ngamma\n".split("\n"));
    const deleted = Chunk.build(original, deletedDoc);
    const delTicks = overviewTicks(deletedDoc, deleted, original);
    expect(delTicks).toHaveLength(1);
    expect(delTicks[0]?.kind).toBe("del");

    const modified = Chunk.build(
      Text.of("alpha\nbeta\ngamma\n".split("\n")),
      Text.of("alpha\nBETA\ngamma\n".split("\n")),
    );
    const modTicks = overviewTicks(
      Text.of("alpha\nBETA\ngamma\n".split("\n")),
      modified,
      null,
    );
    expect(modTicks[0]?.kind).toBe("mod");
  });
});

describe("findChunk", () => {
  it("finds a pure deletion on the following line", () => {
    const original = Text.of("alpha\nbeta\ngamma\n".split("\n"));
    const current = Text.of("alpha\ngamma\n".split("\n"));
    const chunks = Chunk.build(original, current, {
      scanLimit: 5_000,
      timeout: 100,
    });
    const chunk = findChunk(current, chunks, current.line(2).from);
    expect(chunk).toBeTruthy();
    expect(chunk?.fromA).not.toBe(chunk?.toA);
    expect(chunk?.fromB).toBe(chunk?.toB);
  });
});

describe("stateWithGitOriginal", () => {
  it("decorates added, deleted, and modified hunks", () => {
    expect(() =>
      stateWithGitOriginal("alpha\nBETA\ngamma\n", "alpha\nbeta\ngamma\n"),
    ).not.toThrow();
    expect(() =>
      stateWithGitOriginal("alpha\nbeta\ngamma\n", "alpha\ngamma\n"),
    ).not.toThrow();
    expect(() =>
      stateWithGitOriginal("alpha\ngamma\n", "alpha\nbeta\ngamma\n"),
    ).not.toThrow();
    expect(() =>
      stateWithGitOriginal("hello\nworld\n", ""),
    ).not.toThrow();
  });
});

describe("diffChunks - repeated code blocks", () => {
  it("does not shift hunk start for repeated block patterns", () => {
    const block = (name: string, cls: string) => [
      `const ${name} = new (class extends GutterMarker {`,
      "  eq() { return true; }",
      "  toDOM() {",
      '    const el = document.createElement("div");',
      `    el.className = "cm-gitMarker cm-git${cls}";`,
      "    return el;",
      "  }",
      "})();",
      "",
    ];

    const original = block("addMarker", "Add").join("\n");
    const current = [
      ...block("addMarker", "Add"),
      ...block("delMarker", "Del"),
      ...block("modMarker", "Mod"),
    ].join("\n");

    const chunks = diffChunks(original, current);
    expect(chunks).toHaveLength(1);

    const currText = Text.of(current.split("\n"));
    const chunk = chunks[0]!;
    const startLine = currText.lineAt(chunk.fromB).number;
    const endLine = currText.lineAt(Math.min(chunk.endB, currText.length)).number;

    // insertion starts at line 10 (1-based) where delMarker begins,
    // not shifted backward into addMarker's trailing lines
    expect(startLine).toBe(10);
    // insertion ends at line 27 (last line of modMarker block)
    expect(endLine).toBe(27);
  });

  it("matches git diff for simple insertion at end", () => {
    const original = "alpha\nbeta\n";
    const current = "alpha\nbeta\ngamma\n";
    const chunks = diffChunks(original, current);
    expect(chunks).toHaveLength(1);
    const currText = Text.of(current.split("\n"));
    const chunk = chunks[0]!;
    expect(currText.lineAt(chunk.fromB).number).toBe(3);
  });

  it("matches git diff for modification in middle", () => {
    const original = "alpha\nbeta\ngamma\ndelta\n";
    const current = "alpha\nBETA\ngamma\nDELTA\n";
    const chunks = diffChunks(original, current);
    expect(chunks).toHaveLength(2);
    const currText = Text.of(current.split("\n"));
    expect(currText.lineAt(chunks[0]!.fromB).number).toBe(2);
    expect(currText.lineAt(chunks[1]!.fromB).number).toBe(4);
  });
});
