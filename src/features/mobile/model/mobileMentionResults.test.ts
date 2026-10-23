import { expect, it, vi } from "vitest";
import { mobileMentionResults } from "./mobileMentionResults";
import { applyFileMentionsToTurn } from "../../files/model/fileMentions";
vi.mock("../../files/model/fileIndex", async (original) => ({
  ...(await original<typeof import("../../files/model/fileIndex")>()),
  loadProjectFiles: async () => files,
}));
const files = [
  "src/test.ts",
  "mobile/test.ts",
  "docs/read me.md",
  "docs/read-me.md",
].map((relative) => ({
  relative,
  path: `/repo/${relative}`,
  name: relative.split("/").pop()!,
}));
it("ranks files and derived directories exactly as desktop, preserving parent paths and match offsets", () => {
  expect(
    mobileMentionResults(files, "")
      .filter((file) => file.isDir)
      .map((file) => file.path),
  ).toEqual(["docs", "mobile", "src"]);
  const hits = mobileMentionResults(files, "test");
  expect(hits).toHaveLength(2);
  expect(hits.map((file) => file.label)).toEqual([
    "src/test.ts",
    "mobile/test.ts",
  ]);
  expect(hits[0].positions).toEqual([4, 5, 6, 7]);
  expect(
    mobileMentionResults(files, "docs/").some(
      (file) => file.isDir && file.label === "docs",
    ),
  ).toBe(true);
});
it("shows root files and folders for bare @, even when recents are nested", () => {
  const rootFile = {
    path: "/repo/README.md",
    relative: "README.md",
    name: "README.md",
  };
  const hits = mobileMentionResults([...files, rootFile], "", [files[1].path]);
  expect(hits.map((file) => file.path)).toEqual([
    "docs",
    "mobile",
    "src",
    "README.md",
  ]);
  expect(hits.every((file) => !file.path.includes("/"))).toBe(true);
});
it("round-trips spaced-path aliases and directory tokens through desktop prompt preparation", async () => {
  const spaced = mobileMentionResults(files, "read me").find(
    (file) => file.path === "docs/read me.md",
  )!;
  expect(spaced.label).not.toBe("docs/read-me.md"); // Existing real path owns that label.
  const text = `Review @${spaced.label} and @docs and @src/test.ts`;
  expect(await applyFileMentionsToTurn(text, "/repo")).toBe(
    `${text}\n\n---\nReferenced with @ above:\n- @${spaced.label} → docs/read me.md`,
  );
});
