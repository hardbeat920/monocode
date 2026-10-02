import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { newSession, type Session } from "../../sessions/model/session";
import {
  clearComposerDraft,
  setComposerDraft,
} from "../../sessions/model/draftCache";
import {
  leaf,
  leafIds,
  newFileTab,
  newTab,
  newTerminalFile,
  openEditorTab,
  openTerminalTab,
  splitPane,
  type WorkspaceTab,
} from "../../workspace/model/layout";
import { closeAgentSession } from "./closeSession";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const cwd = "/projects/monocode";
function session(id: string): Session {
  return {
    ...newSession("codex", cwd),
    id,
    blocks: [
      { id: `${id}-user`, role: "user", text: "Keep this conversation" },
    ],
  };
}
function fixture(tabs?: WorkspaceTab[]) {
  const source = { ...session("source"), busy: true };
  const target = {
    ...session("target"),
    branch: "feature",
    worktreeCwd: "/trees/feature",
  };
  let state = {
    sessions: [source, target],
    tabs: tabs ?? [newTab(source.id), newTab(target.id)],
    activeTabId: "",
  };
  state.activeTabId = state.tabs.at(-1)!.id;
  const workspace = {
    snapshot: () => state,
    unavailable: vi.fn(() => false),
    worktreeOf: () => null,
    apply: vi.fn((next: typeof state) => {
      state = next;
    }),
  };
  return {
    source,
    target,
    workspace,
    close: () => closeAgentSession(source, target.id, workspace),
  };
}

beforeEach(() => {
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockImplementation(async (command, args: any) => {
    if (command !== "session_upsert")
      throw new Error(`Unexpected side effect: ${command}`);
    return { ...args.session, createdAt: 1, updatedAt: 2 };
  });
});
afterEach(() => clearComposerDraft("target"));

describe("closeAgentSession lifecycle", () => {
  it("saves the real transcript and draft before closing, with a harmless retry", async () => {
    const f = fixture();
    f.target.blocks.push({
      id: "draft",
      role: "user",
      text: "Unsent",
      draft: true,
    });
    const original = structuredClone(f.target);
    let finish!: (value: unknown) => void;
    vi.mocked(invoke).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    const closing = f.close();
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledOnce());
    expect(f.workspace.apply).not.toHaveBeenCalled();
    const payload = vi.mocked(invoke).mock.calls[0][1] as { session: Session };
    expect(payload.session).toMatchObject({
      blocks: original.blocks,
      branch: "feature",
      worktreeCwd: "/trees/feature",
    });
    finish({ ...payload.session, createdAt: 1, updatedAt: 2 });
    await expect(closing).resolves.toEqual({ closed: true });
    expect(
      f.workspace.snapshot().tabs.map((tab) => leafIds(tab.layout)),
    ).toEqual([["source"]]);
    expect(f.workspace.snapshot().activeTabId).toBe(
      f.workspace.snapshot().tabs[0].id,
    );
    expect(f.target).toEqual(original);
    expect(f.source.busy).toBe(true);
    await expect(f.close()).resolves.toEqual({ closed: false });
    expect(invoke).toHaveBeenCalledOnce();
  });

  it("removes every target leaf while keeping nested siblings, files and terminals", async () => {
    let shared = {
      ...newTab("source"),
      layout: splitPane(leaf("source"), "source", "right", "target"),
    };
    shared = openEditorTab(shared, newFileTab(`${cwd}/unsaved.txt`, cwd));
    shared = openTerminalTab(
      shared,
      newTerminalFile(cwd, "Terminal"),
    );
    const other = {
      ...newTab("target"),
      layout: splitPane(leaf("target"), "target", "down", "neighbor"),
    };
    const f = fixture([shared, other, newTab("target")]);
    f.workspace
      .snapshot()
      .sessions.push({ ...session("neighbor"), busy: true });
    const before = structuredClone(f.workspace.snapshot());
    await f.close();
    const after = f.workspace.snapshot();
    expect(after.tabs).toHaveLength(2);
    expect(after.tabs.flatMap((tab) => leafIds(tab.layout))).not.toContain(
      "target",
    );
    expect(after.tabs[0].editorPanes).toEqual(before.tabs[0].editorPanes);
    expect(after.tabs[0].terminalPanes).toEqual(before.tabs[0].terminalPanes);
    expect(after.tabs[1].layout).toEqual(leaf("neighbor"));
    expect(after.tabs[1].focusedId).toBe("neighbor");
    expect(after.sessions).toEqual(before.sessions);
    expect(invoke).toHaveBeenCalledTimes(1);
  });

  it("keeps file-only and terminal-only remainder panes instead of closing their tab", async () => {
    const tab = openTerminalTab(
      openEditorTab(newTab("target"), newFileTab(`${cwd}/a`, cwd)),
      newTerminalFile(cwd, "Terminal"),
    );
    const f = fixture([newTab("source"), tab]);
    await f.close();
    const remaining = f.workspace.snapshot().tabs[1];
    expect(remaining.editorPanes).toEqual(tab.editorPanes);
    expect(remaining.terminalPanes).toEqual(tab.terminalPanes);
    expect(leafIds(remaining.layout)).not.toContain("target");
    expect(leafIds(remaining.layout)).toContain(remaining.focusedId);
    expect(f.workspace.snapshot().activeTabId).toBe(tab.id);
  });

  it("replaces the last tab in the project/worktree without leaving a dangling focus", async () => {
    const f = fixture([newTab("target")]);
    await f.close();
    const state = f.workspace.snapshot();
    const replacement = state.sessions.find(
      (entry) => entry.id === state.tabs[0].focusedId,
    )!;
    expect(replacement.id).not.toBe("target");
    expect(replacement).toMatchObject({
      cwd,
      worktreeCwd: "/trees/feature",
      blocks: [],
    });
    expect(state.activeTabId).toBe(state.tabs[0].id);
    expect(state.sessions).toContain(f.target);
  });

  it.each([new Error("disk full"), null])(
    "leaves every view open when persistence fails: %s",
    async (failure) => {
      const f = fixture();
      if (failure) vi.mocked(invoke).mockRejectedValueOnce(failure);
      else vi.mocked(invoke).mockResolvedValueOnce(null);
      await expect(f.close()).rejects.toThrow(
        failure ? "disk full" : "could not be saved",
      );
      expect(f.workspace.apply).not.toHaveBeenCalled();
    },
  );

  it.each([
    { busy: true },
    { worktreePreparing: true },
    { backgroundTasks: ["task"] },
    { queuedMessages: [{ id: "queued", text: "later", attachments: [] }] },
    { pendingQuestion: { requestId: 1, questions: [] } },
  ])(
    "refuses in-flight work without persistence or interruption: %j",
    async (fields) => {
      const f = fixture();
      Object.assign(f.target, fields);
      await expect(f.close()).rejects.toThrow("busy");
      expect(invoke).not.toHaveBeenCalled();
      expect(f.workspace.apply).not.toHaveBeenCalled();
    },
  );

  it.each(["busy", "draft", "transcript", "project", "unavailable"])(
    "rechecks %s after awaiting persistence",
    async (change) => {
      const f = fixture();
      vi.mocked(invoke).mockImplementationOnce(async (_command, args: any) => {
        if (change === "busy") f.target.busy = true;
        if (change === "draft")
          setComposerDraft("target", "Typing while saving");
        if (change === "transcript")
          f.target.blocks = [
            ...f.target.blocks,
            { id: "new", role: "user", text: "new" },
          ];
        if (change === "project") f.target.cwd = "/other";
        if (change === "unavailable")
          f.workspace.unavailable.mockReturnValue(true);
        return { ...args.session, createdAt: 1, updatedAt: 2 };
      });
      await expect(f.close()).rejects.toThrow();
      expect(f.workspace.apply).not.toHaveBeenCalled();
    },
  );

  it("uses the current layout after saving instead of a stale tab snapshot", async () => {
    const f = fixture();
    vi.mocked(invoke).mockImplementationOnce(async (_command, args: any) => {
      f.workspace.snapshot().tabs = [
        {
          ...newTab("source"),
          layout: splitPane(leaf("source"), "source", "down", "target"),
        },
      ];
      f.workspace.snapshot().activeTabId = f.workspace.snapshot().tabs[0].id;
      return { ...args.session, createdAt: 1, updatedAt: 2 };
    });
    await f.close();
    expect(f.workspace.snapshot().tabs[0].layout).toEqual(leaf("source"));
  });

  it("refuses cached unsent text with an actionable error", async () => {
    const f = fixture();
    setComposerDraft("target", "Do not lose this");
    await expect(f.close()).rejects.toThrow("/draft");
    expect(f.workspace.apply).not.toHaveBeenCalled();
    expect(invoke).not.toHaveBeenCalled();
  });

  it.each([
    { cwd: "/other" },
    { orchestrationLeadId: "lead" },
    { inboxAsk: {} },
  ])("rejects inaccessible live targets: %j", async (fields) => {
    const f = fixture();
    Object.assign(f.target, fields);
    await expect(f.close()).rejects.toThrow("unavailable");
    expect(invoke).not.toHaveBeenCalled();
  });

  it("refuses the caller and a still-loading view", async () => {
    const f = fixture();
    await expect(
      closeAgentSession(f.source, f.source.id, f.workspace),
    ).rejects.toThrow("calling session");
    f.workspace.snapshot().sessions = [f.source];
    await expect(f.close()).rejects.toThrow("still loading");
    expect(invoke).not.toHaveBeenCalled();
  });
});
