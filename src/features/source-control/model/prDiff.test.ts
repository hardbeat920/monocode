import { describe, expect, it } from "vitest";
import { mergePrDiff, parsePrPatch } from "./prDiff";

const SAMPLE = `diff --git a/next.config.ts b/next.config.ts
index 1111111..2222222 100644
--- a/next.config.ts
+++ b/next.config.ts
@@ -1,6 +1,10 @@
 import type { NextConfig } from "next";

 const nextConfig: NextConfig = {
+  cacheComponents: true,
+  partialPrefetching: true,
   transpilePackages: ["gl-transition"],
 };
`;

describe("parsePrPatch", () => {
  it("reads added lines and line numbers", () => {
    const files = parsePrPatch(SAMPLE);
    expect(files).toHaveLength(1);
    expect(files[0].path).toBe("next.config.ts");
    expect(files[0].status).toBe("modified");
    expect(files[0].additions).toBe(2);
    expect(files[0].deletions).toBe(0);
    const added = files[0].lines.filter((line) => line.kind === "add");
    expect(added.map((line) => line.text)).toEqual([
      "  cacheComponents: true,",
      "  partialPrefetching: true,",
    ]);
    expect(added[0].newNumber).toBe(4);
    expect(added[0].oldNumber).toBeNull();
  });

  it("marks new and deleted files", () => {
    const files = parsePrPatch(`diff --git a/new.txt b/new.txt
new file mode 100644
index 0000000..abc
--- /dev/null
+++ b/new.txt
@@ -0,0 +1,2 @@
+hello
+world
diff --git a/gone.txt b/gone.txt
deleted file mode 100644
index abc..0000000
--- a/gone.txt
+++ /dev/null
@@ -1,1 +0,0 @@
-bye
`);
    expect(files.map((file) => [file.path, file.status, file.additions, file.deletions])).toEqual([
      ["new.txt", "added", 2, 0],
      ["gone.txt", "deleted", 0, 1],
    ]);
  });

  it("reads renames and binary files", () => {
    const files = parsePrPatch(`diff --git a/old.ts b/new.ts
similarity index 90%
rename from old.ts
rename to new.ts
diff --git a/photo.png b/photo.png
index 111..222
Binary files a/photo.png and b/photo.png differ
`);
    expect(files[0]).toMatchObject({
      path: "new.ts",
      previousPath: "old.ts",
      status: "renamed",
    });
    expect(files[1]).toMatchObject({
      path: "photo.png",
      binary: true,
      lines: [],
    });
  });

  it("returns an empty list for a blank patch", () => {
    expect(parsePrPatch("")).toEqual([]);
    expect(parsePrPatch("   \n")).toEqual([]);
  });
});

describe("mergePrDiff", () => {
  it("prefers GitHub file counts when the patch parsed", () => {
    const merged = mergePrDiff(
      [{ path: "next.config.ts", additions: 4, deletions: 0 }],
      parsePrPatch(SAMPLE),
    );
    expect(merged[0].additions).toBe(4);
    expect(merged[0].lines.some((line) => line.kind === "add")).toBe(true);
  });

  it("keeps GitHub file order", () => {
    const merged = mergePrDiff(
      [
        { path: "b.ts", additions: 1, deletions: 0 },
        { path: "a.ts", additions: 1, deletions: 0 },
      ],
      parsePrPatch(`diff --git a/a.ts b/a.ts
--- a/a.ts
+++ b/a.ts
@@ -1 +1,2 @@
 keep
+a
diff --git a/b.ts b/b.ts
--- a/b.ts
+++ b/b.ts
@@ -1 +1,2 @@
 keep
+b
`),
    );
    expect(merged.map((file) => file.path)).toEqual(["b.ts", "a.ts"]);
  });

  it("falls back to the file list when there is no patch", () => {
    const merged = mergePrDiff(
      [{ path: "secret.bin", additions: 0, deletions: 0 }],
      [],
    );
    expect(merged).toEqual([
      {
        path: "secret.bin",
        previousPath: null,
        status: "modified",
        binary: false,
        additions: 0,
        deletions: 0,
        lines: [],
      },
    ]);
  });

  it("keeps GitHub file order and includes files missing from the patch", () => {
    const merged = mergePrDiff(
      [
        { path: "package.json", additions: 2, deletions: 2 },
        { path: "next.config.ts", additions: 4, deletions: 0 },
      ],
      parsePrPatch(SAMPLE),
    );
    expect(merged.map((file) => file.path)).toEqual([
      "package.json",
      "next.config.ts",
    ]);
    expect(merged[0].lines).toEqual([]);
    expect(merged[1].additions).toBe(4);
    expect(merged[1].lines.some((line) => line.kind === "add")).toBe(true);
  });
});
