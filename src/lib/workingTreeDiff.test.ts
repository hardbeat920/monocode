import { describe, expect, it } from "vitest";
import type { GitChangedFile } from "./fs";
import {
  prioritizeWorkingTreeDiffEntries,
  workingTreeDiffEntries,
  workingTreeDiffEntryLabel,
  workingTreeDiffFocusId,
} from "./workingTreeDiff";

function file(
  relative: string,
  staged: boolean,
  unstaged: boolean,
): GitChangedFile {
  return {
    path: `/repo/${relative}`,
    relative,
    status: "modified",
    additions: 1,
    deletions: 0,
    staged,
    unstaged,
  };
}

describe("workingTreeDiffEntries", () => {
  it("creates a staged entry for a clean staged file", () => {
    const entries = workingTreeDiffEntries([file("a.ts", true, false)]);
    expect(entries.map(({ id, kind }) => ({ id, kind }))).toEqual([
      { id: "staged:a.ts", kind: "staged" },
    ]);
  });

  it("creates both comparisons for a partially staged file", () => {
    const entries = workingTreeDiffEntries([file("a.ts", true, true)]);
    expect(entries.map((entry) => entry.id)).toEqual([
      "staged:a.ts",
      "unstaged:a.ts",
    ]);
    expect(entries.map(workingTreeDiffEntryLabel)).toEqual([
      "a.ts (Staged)",
      "a.ts (Unstaged)",
    ]);
  });

  it("focuses and prioritizes the selected comparison", () => {
    const entries = workingTreeDiffEntries([
      file("a.ts", true, true),
      file("b.ts", false, true),
    ]);
    expect(workingTreeDiffFocusId(entries, "/repo/a.ts", "unstaged")).toBe(
      "unstaged:a.ts",
    );
    expect(
      prioritizeWorkingTreeDiffEntries(entries, "/repo/a.ts", "unstaged")[0]
        ?.id,
    ).toBe("unstaged:a.ts");
  });
});
