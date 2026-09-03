import { describe, expect, it } from "vitest";
import { newChangesTab, newCommitTab, newReleaseNotesWorkspaceTab } from "../lib/layout";
import { releaseNotesTitle } from "../lib/releaseNotes";
import { appendProblems, surfaceTabPresentation } from "./SurfaceTabs";

describe("surfaceTabPresentation", () => {
  it("labels release notes from their version", () => {
    const file = newReleaseNotesWorkspaceTab({ version: "0.1.23" })
      .editorPanes[0]?.files[0];
    if (!file) throw new Error("expected release-note file");

    expect(surfaceTabPresentation(file)).toEqual({
      name: releaseNotesTitle("0.1.23"),
      label: releaseNotesTitle("0.1.23"),
      iconName: "CHANGELOG.md",
      tooltip: releaseNotesTitle("0.1.23"),
    });
  });

  it("labels the unified working-tree tab as Changes", () => {
    expect(surfaceTabPresentation(newChangesTab("/repo", "/repo/App.tsx"))).toEqual({
      name: "Changes",
      label: "Changes",
      iconName: "CHANGES",
      tooltip: "Working tree changes",
    });
  });

  it("labels a commit tab from the subject", () => {
    expect(
      surfaceTabPresentation(
        newCommitTab("/repo", {
          sha: "abc1234deadbeef",
          shortSha: "abc1234",
          subject: "Fix the graph",
        }),
      ),
    ).toEqual({
      name: "Fix the graph",
      label: "Fix the graph",
      iconName: "CHANGES",
      tooltip: "abc1234 — Fix the graph",
    });
  });
});

describe("appendProblems", () => {
  it("leaves a clean file's tooltip alone", () => {
    expect(appendProblems("/repo/src/app.ts", 0)).toBe("/repo/src/app.ts");
  });

  it("singularises a lone problem", () => {
    expect(appendProblems("/repo/src/app.ts", 1)).toBe(
      "/repo/src/app.ts — 1 problem",
    );
  });

  it("pluralises the rest", () => {
    expect(appendProblems("/repo/src/app.ts", 4)).toBe(
      "/repo/src/app.ts — 4 problems",
    );
  });
});
