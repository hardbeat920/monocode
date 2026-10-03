import { describe, expect, it } from "vitest";
import type { GitChangedFile } from "../../../platform/tauri/fs";
import { visibleChangeOrder } from "./changeTree";

function file(relative: string): GitChangedFile {
  return {
    path: `/repo/${relative}`,
    relative,
    status: "modified",
    additions: 0,
    deletions: 0,
    staged: false,
    unstaged: true,
  };
}

const files = [
  "z.ts",
  "src/b.ts",
  "src/lib/util.ts",
  "a.ts",
  "docs/readme.md",
  "src/a.ts",
  "src/lib/deep/x.ts",
].map(file);

const none = () => false;

describe("visibleChangeOrder", () => {
  it("keeps the given order in list view", () => {
    expect(visibleChangeOrder(files, "list", () => true)).toEqual(
      files.map((f) => f.relative),
    );
  });

  it("orders the tree folders first, then files by basename", () => {
    expect(visibleChangeOrder(files, "tree", none)).toEqual([
      "docs/readme.md",
      "src/lib/deep/x.ts",
      "src/lib/util.ts",
      "src/a.ts",
      "src/b.ts",
      "a.ts",
      "z.ts",
    ]);
  });

  it("hides every descendant of a collapsed folder", () => {
    expect(visibleChangeOrder(files, "tree", (dir) => dir === "src")).toEqual([
      "docs/readme.md",
      "a.ts",
      "z.ts",
    ]);
  });

  it("hides only the collapsed nested folder, not its siblings", () => {
    expect(
      visibleChangeOrder(files, "tree", (dir) => dir === "src/lib"),
    ).toEqual(["docs/readme.md", "src/a.ts", "src/b.ts", "a.ts", "z.ts"]);
  });

  it("keeps a collapsed folder's siblings visible", () => {
    expect(visibleChangeOrder(files, "tree", (dir) => dir === "docs")).toEqual([
      "src/lib/deep/x.ts",
      "src/lib/util.ts",
      "src/a.ts",
      "src/b.ts",
      "a.ts",
      "z.ts",
    ]);
  });

  it("returns nothing without files", () => {
    expect(visibleChangeOrder([], "tree", none)).toEqual([]);
  });
});
