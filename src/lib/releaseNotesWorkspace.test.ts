import { describe, expect, it } from "vitest";
import {
  leaf,
  newEditorPane,
  newFileTab,
  newReleaseNotesWorkspaceTab,
  newTab,
  splitPane,
  type WorkspaceTab,
} from "./layout";
import {
  focusReleaseNotesTarget,
  planReleaseNotesOpen,
} from "./releaseNotesWorkspace";

function tabWithHiddenRelease(): WorkspaceTab {
  const tab = newReleaseNotesWorkspaceTab({ version: "0.1.23" });
  const releasePane = tab.editorPanes[0];
  if (!releasePane) throw new Error("expected release pane");
  const activeFile = newFileTab("/repo/App.tsx", "/repo");
  const otherPane = newEditorPane(newFileTab("/repo/Other.tsx", "/repo"));
  return {
    ...tab,
    layout: splitPane(
      leaf(releasePane.id),
      releasePane.id,
      "right",
      otherPane.id,
    ),
    focusedId: otherPane.id,
    editorPanes: [
      {
        ...releasePane,
        files: [...releasePane.files, activeFile],
        activeFileId: activeFile.id,
      },
      otherPane,
    ],
  };
}

describe("planReleaseNotesOpen", () => {
  it("opens when the release is missing", () => {
    expect(planReleaseNotesOpen([newTab("session-a")], "0.1.23")).toEqual({
      kind: "open",
    });
  });

  it("identifies the exact release tab, pane, and file", () => {
    const tab = tabWithHiddenRelease();
    const releasePane = tab.editorPanes[0]!;
    const releaseFile = releasePane.files[0]!;

    expect(planReleaseNotesOpen([tab], "0.1.23")).toEqual({
      kind: "focus",
      tabId: tab.id,
      paneId: releasePane.id,
      fileId: releaseFile.id,
    });
  });
});

describe("focusReleaseNotesTarget", () => {
  it("reveals an existing release without adding a duplicate", () => {
    const tab = tabWithHiddenRelease();
    const target = planReleaseNotesOpen([tab], "0.1.23");
    if (target.kind !== "focus") throw new Error("expected focus plan");

    const result = focusReleaseNotesTarget([tab], target);

    expect(result).toHaveLength(1);
    expect(result[0]?.focusedId).toBe(target.paneId);
    expect(result[0]?.editorPanes[0]?.activeFileId).toBe(target.fileId);
    expect(
      result[0]?.editorPanes
        .flatMap((pane) => pane.files)
        .filter((file) => file.releaseNotes?.version === "0.1.23"),
    ).toHaveLength(1);
  });
});
