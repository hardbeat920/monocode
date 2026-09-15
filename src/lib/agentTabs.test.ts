import { describe, expect, it } from "vitest";
import {
  editorTabKey,
  isAgentTab,
  isFilesystemTab,
  leafIds,
  newAgentTab,
  newFileTab,
  newTab,
  openEditorTab,
} from "./layout";
import { groupTurns } from "../surfaces/transcriptActivity";
import type { Block } from "./session";

const agent = (sessionId: string, title: string) =>
  newAgentTab(title, "/repo", {
    sessionId,
    leadId: "lead",
    harness: "codex",
  });

describe("agent tabs", () => {
  it("gathers every agent of a run into one pane beside the lead", () => {
    let tab = newTab("lead");
    tab = openEditorTab(tab, agent("worker-a", "Audit the engine"));
    const paneId = tab.editorPanes[0].id;
    tab = openEditorTab(tab, agent("worker-b", "Audit the UI"));

    expect(tab.editorPanes).toHaveLength(1);
    expect(tab.editorPanes[0].id).toBe(paneId);
    expect(tab.editorPanes[0].files.map((file) => file.path)).toEqual([
      "Audit the engine",
      "Audit the UI",
    ]);
    // The pane sits beside the lead, which keeps its own leaf.
    expect(leafIds(tab.layout)).toEqual(["lead", paneId]);
    expect(tab.focusedId).toBe(paneId);
  });

  it("focuses the tab an agent already has instead of opening a second", () => {
    let tab = newTab("lead");
    tab = openEditorTab(tab, agent("worker-a", "Audit the engine"));
    tab = openEditorTab(tab, agent("worker-b", "Audit the UI"));
    const first = tab.editorPanes[0].files[0];
    tab = openEditorTab(tab, agent("worker-a", "Audit the engine"));

    expect(tab.editorPanes[0].files).toHaveLength(2);
    expect(tab.editorPanes[0].activeFileId).toBe(first.id);
  });

  it("is keyed by its worker, and is not a file on disk", () => {
    const file = agent("worker-a", "Audit the engine");
    expect(editorTabKey(file)).toBe("agent:worker-a");
    expect(isAgentTab(file)).toBe(true);
    expect(file.agent.harness).toBe("codex");
    expect(isFilesystemTab(file)).toBe(false);
    expect(isFilesystemTab(newFileTab("/repo/a.ts", "/repo"))).toBe(true);
  });
});

describe("a worker's own transcript", () => {
  const blocks: Block[] = [
    { id: "u1", role: "user", text: "Audit the engine", internal: true },
    { id: "a1", role: "assistant", text: "Looking now" },
    { id: "u2", role: "user", text: "Also check retries", internal: true },
    { id: "a2", role: "assistant", text: "Done" },
  ];

  it("shows the orchestrator's turns, which the lead's own transcript hides", () => {
    expect(
      groupTurns(blocks, true).map((turn) => turn.map((b) => b.id)),
    ).toEqual([
      ["u1", "a1"],
      ["u2", "a2"],
    ]);
    // Without it the prompts vanish and the replies fold into one turn, which
    // is what left the worker's answers with nothing above them.
    expect(groupTurns(blocks).map((turn) => turn.map((b) => b.id))).toEqual([
      ["a1", "a2"],
    ]);
  });
});
