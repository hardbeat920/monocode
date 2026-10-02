import { describe, expect, it } from "vitest";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { browseHostDirectories } from "./browse";

describe("host directory browser", () => {
  it("lists folders without exposing files and rejects relative paths", async () => {
    const root = mkdtempSync(join(tmpdir(), "monocode-browse-"));
    try {
      mkdirSync(join(root, "repo"));
      writeFileSync(join(root, "secret.txt"), "private data");
      const result = await browseHostDirectories(root);
      expect(result.path).toBe(root);
      expect(result.entries).toEqual([
        { name: "repo", path: join(root, "repo") },
      ]);
      expect(result.parent).not.toBeNull();
      await expect(browseHostDirectories("relative/path")).rejects.toThrow(
        "absolute",
      );
      await expect(browseHostDirectories("bad\0path")).rejects.toThrow(
        "Invalid",
      );
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });
});
