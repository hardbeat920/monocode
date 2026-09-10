import { describe, expect, it } from "vitest";
import { INTERRUPT_MESSAGE } from "./inFlight";
import {
  leaf,
  newChangesTab,
  newCommitTab,
  newFileTab,
  newReleaseNotesWorkspaceTab,
  newSessionChangesTab,
  newTab,
  newTerminalFile,
} from "./layout";
import { createProjectTerminal } from "./projectTerminal";
import { newSession, type Session } from "./session";
import {
  collectWorkspaceSnapshot,
  hydrateWorkspaceSnapshot,
  parseWorkspaceSnapshot,
} from "./workspaceSnapshot";

function chat(id: string, cwd: string): Session {
  const session = newSession("cursor", cwd);
  session.id = id;
  session.blocks = [{ id: "u1", role: "user", text: "hello" }];
  session.providerSessionId = "p1";
  return session;
}

describe("project return snapshots", () => {
  function saved() {
    const sessions = [
      chat("a1", "/alpha"),
      chat("a2", "/alpha"),
      chat("b1", "/beta"),
      chat("b2", "/beta"),
    ];
    const tabs = sessions.map((session) => ({
      ...newTab(session.id),
      id: `tab-${session.id}`,
    }));
    return {
      ...collectWorkspaceSnapshot(tabs, sessions, "tab-b2", "/beta", new Map()),
      projectReturnTargets: [
        { projectPath: "/alpha", tabId: "a2" },
        { projectPath: "/beta", tabId: "b2" },
      ],
    };
  }

  it("collects and round-trips choices while rejecting stale references", () => {
    const sessions = [
      chat("a1", "/alpha"),
      chat("a2", "/alpha"),
      chat("b2", "/beta"),
    ];
    const tabs = sessions.map((session) => ({
      ...newTab(session.id),
      id: `tab-${session.id}`,
    }));
    const memory = new Map([
      ["/alpha", "a2"],
      ["/beta", "b2"],
      ["/gone", "missing"],
    ]);
    const snapshot = collectWorkspaceSnapshot(
      tabs,
      sessions,
      "tab-b2",
      "/beta",
      memory,
    );
    const restored = hydrateWorkspaceSnapshot(snapshot, new Map());
    expect([...(restored?.projectReturnMemory ?? [])]).toEqual([
      ["/alpha", "a2"],
      ["/beta", "b2"],
    ]);
    expect(memory.size).toBe(3);
  });

  it("restores both project choices, not just the active tab", () => {
    const restored = hydrateWorkspaceSnapshot(saved(), new Map());
    expect(restored?.projectReturnMemory?.get("/alpha")).toBe("a2");
    expect(restored?.projectReturnMemory?.get("/beta")).toBe("b2");
  });

  it("loads old snapshots and seeds only the active project", () => {
    const { projectReturnTargets: _targets, ...old } = saved();
    const restored = hydrateWorkspaceSnapshot(old, new Map());
    expect([...(restored?.projectReturnMemory ?? [])]).toEqual([
      ["/beta", "b2"],
    ]);
  });

  it("prunes invalid entries and uses the last valid duplicate", () => {
    const raw = saved();
    const parsed = parseWorkspaceSnapshot({
      ...raw,
      projectReturnTargets: [
        null,
        42,
        {},
        { projectPath: "/alpha", tabId: 12 },
        { projectPath: "/alpha/", tabId: "a1" },
        { projectPath: "/alpha", tabId: "a2" },
        { projectPath: "/gone", tabId: "missing" },
        { projectPath: "/beta", tabId: "a1" },
      ],
    });
    expect(parsed?.projectReturnTargets).toEqual([
      { projectPath: "/alpha", tabId: "a2" },
    ]);
  });

  it("lets the restored active tab override inconsistent saved preference", () => {
    const raw = saved();
    raw.projectReturnTargets[1].tabId = "tab-b1";
    expect(
      hydrateWorkspaceSnapshot(raw, new Map())?.projectReturnMemory?.get(
        "/beta",
      ),
    ).toBe("b2");
  });

  it.each([null, "broken", {}])(
    "ignores a malformed choice list: %j",
    (projectReturnTargets) => {
      const restored = hydrateWorkspaceSnapshot(
        { ...saved(), projectReturnTargets },
        new Map(),
      );
      expect(restored?.tabs).toHaveLength(4);
      expect([...(restored?.projectReturnMemory ?? [])]).toEqual([
        ["/beta", "b2"],
      ]);
    },
  );

  it("rechecks project membership against loaded sessions", () => {
    const restored = hydrateWorkspaceSnapshot(
      saved(),
      new Map([["a2", chat("a2", "/moved")]]),
    );
    expect(restored?.projectReturnMemory?.has("/alpha")).toBe(false);
    expect(restored?.projectReturnMemory?.get("/beta")).toBe("b2");
  });
});

describe("collectWorkspaceSnapshot", () => {
  it("stores tabs, stubs, and the focused tab — not transcripts", () => {
    const session = chat("s1", "/tmp/a");
    session.blocks.push({ id: "a1", role: "assistant", text: "hi" });
    const file = newFileTab("/tmp/a/README.md", "/tmp/a");
    const tab = {
      ...newTab("s1"),
      id: "t1",
      editorPanes: [{ id: "e1", files: [file], activeFileId: file.id }],
    };
    const snapshot = collectWorkspaceSnapshot(
      [tab],
      [session],
      "t1",
      "/tmp/a",
      new Map(),
    );
    expect(snapshot.activeTabId).toBe("t1");
    expect(snapshot.tabs[0]?.editorPanes[0]?.files[0]?.path).toBe(
      "/tmp/a/README.md",
    );
    expect(snapshot.sessions).toEqual([
      expect.objectContaining({
        id: "s1",
        cwd: "/tmp/a",
        providerSessionId: "p1",
      }),
    ]);
    expect("blocks" in snapshot.sessions[0]!).toBe(false);
    expect(snapshot.projectTerminals).toEqual([]);
  });

  it("round-trips a unified Changes tab", () => {
    const file = newChangesTab("/tmp/a", "/tmp/a/src/lib.rs", "staged");
    const tab = {
      ...newTab("s1"),
      id: "t1",
      editorPanes: [{ id: "e1", files: [file], activeFileId: file.id }],
    };
    const snapshot = collectWorkspaceSnapshot(
      [tab],
      [],
      "t1",
      "/tmp/a",
      new Map(),
    );
    const workspace = hydrateWorkspaceSnapshot(snapshot, new Map());
    const restored = workspace?.tabs[0]?.editorPanes[0]?.files[0];
    expect(restored?.changes).toBe(true);
    expect(restored?.review).toBe(true);
    expect(restored?.path).toBe("/tmp/a/src/lib.rs");
    expect(restored?.changeKind).toBe("staged");
  });

  it("round-trips a session-scoped Changes tab", () => {
    const file = newSessionChangesTab(
      "/tmp/a",
      "session-a",
      "/tmp/a/src/lib.rs",
    );
    const tab = {
      ...newTab("s1"),
      id: "t1",
      editorPanes: [{ id: "e1", files: [file], activeFileId: file.id }],
    };
    const snapshot = collectWorkspaceSnapshot(
      [tab],
      [],
      "t1",
      "/tmp/a",
      new Map(),
    );
    const restored = hydrateWorkspaceSnapshot(snapshot, new Map())?.tabs[0]
      ?.editorPanes[0]?.files[0];
    expect(restored?.sessionChanges).toEqual({ sessionId: "session-a" });
    expect(restored?.review).toBe(true);
    expect(restored?.path).toBe("/tmp/a/src/lib.rs");
  });

  it("round-trips a commit review tab", () => {
    const file = newCommitTab("/tmp/a", {
      sha: "abc1234deadbeef",
      shortSha: "abc1234",
      subject: "Fix the graph",
    });
    const tab = {
      ...newTab("s1"),
      id: "t1",
      editorPanes: [{ id: "e1", files: [file], activeFileId: file.id }],
    };
    const snapshot = collectWorkspaceSnapshot(
      [tab],
      [],
      "t1",
      "/tmp/a",
      new Map(),
    );
    const workspace = hydrateWorkspaceSnapshot(snapshot, new Map());
    const restored = workspace?.tabs[0]?.editorPanes[0]?.files[0];
    expect(restored?.commit).toEqual({
      sha: "abc1234deadbeef",
      shortSha: "abc1234",
      subject: "Fix the graph",
    });
  });

  it("round-trips a release-note descriptor", () => {
    const tab = newReleaseNotesWorkspaceTab({ version: "0.1.22" });
    const snapshot = collectWorkspaceSnapshot(
      [tab],
      [],
      tab.id,
      "~",
      new Map(),
    );
    const workspace = hydrateWorkspaceSnapshot(snapshot, new Map());

    expect(workspace?.tabs[0]?.editorPanes[0]?.files[0]?.releaseNotes).toEqual({
      version: "0.1.22",
    });
    expect(workspace?.sessions).toEqual([]);
  });

  it("stores the project terminal dock", () => {
    const term = newTerminalFile("/tmp/a", "zsh");
    const dock = createProjectTerminal("/tmp/a", term);
    const snapshot = collectWorkspaceSnapshot(
      [{ ...newTab("s1"), id: "t1" }],
      [],
      "t1",
      "/tmp/a",
      new Map(),
      [dock],
    );
    expect(snapshot.projectTerminals).toEqual([
      expect.objectContaining({
        projectPath: "/tmp/a",
        side: "bottom",
        open: true,
      }),
    ]);
    expect(snapshot.projectTerminals[0]?.pane.files[0]?.id).toBe(term.id);
  });
});

describe("parseWorkspaceSnapshot", () => {
  it("returns null for empty or invalid payloads", () => {
    expect(parseWorkspaceSnapshot(null)).toBeNull();
    expect(parseWorkspaceSnapshot({ tabs: [], activeTabId: "t1" })).toBeNull();
    expect(
      parseWorkspaceSnapshot({ tabs: [{}], activeTabId: "t1" }),
    ).toBeNull();
  });

  it("drops unknown fields and repairs a missing active tab", () => {
    const tab = { ...newTab("s1"), id: "t1" };
    const parsed = parseWorkspaceSnapshot({
      tabs: [{ ...tab, extra: true }],
      sessions: [
        {
          id: "s1",
          harness: "cursor",
          runtimeMode: "supervised",
          cwd: "/tmp/a",
        },
      ],
      activeTabId: "missing",
      projectCwd: "/tmp/a",
    });
    expect(parsed?.activeTabId).toBe("t1");
    expect(parsed?.tabs[0] && "extra" in parsed.tabs[0]).toBe(false);
  });

  it.each([
    { releaseNotes: { version: "" } },
    { releaseNotes: { version: 123 } },
    {
      releaseNotes: { version: "0.1.22" },
      plan: { sessionId: "s", blockId: "b", title: "Plan" },
    },
    { releaseNotes: { version: "0.1.22" }, review: true },
    { releaseNotes: { version: "0.1.22" }, changes: true },
    { releaseNotes: { version: "0.1.22" }, terminal: true },
    {
      releaseNotes: { version: "0.1.22" },
      commit: { sha: "abc", shortSha: "abc", subject: "x" },
    },
  ])("rejects a tab whose release pane is invalid: %j", (descriptor) => {
    const valid = { ...newTab("session-a"), id: "valid-tab" };
    const invalidPaneId = "invalid-release-pane";
    const invalid = {
      kind: "session",
      id: "invalid-tab",
      layout: leaf(invalidPaneId),
      focusedId: invalidPaneId,
      editorPanes: [
        {
          id: invalidPaneId,
          activeFileId: "release-file",
          files: [
            {
              id: "release-file",
              path: "release-notes:0.1.22",
              cwd: "~",
              ...descriptor,
            },
          ],
        },
      ],
      terminalPanes: [],
    };

    const parsed = parseWorkspaceSnapshot({
      tabs: [valid, invalid],
      sessions: [],
      activeTabId: "invalid-tab",
      projectCwd: "~",
    });
    expect(parsed?.tabs.map((tab) => tab.id)).toEqual(["valid-tab"]);

    const workspace = parsed && hydrateWorkspaceSnapshot(parsed, new Map());
    expect(
      workspace?.sessions.some((session) => session.id === invalidPaneId),
    ).toBe(false);
  });
});

describe("hydrateWorkspaceSnapshot", () => {
  it("reopens splits, file panes, and stored transcripts", () => {
    const left = chat("s1", "/tmp/a");
    const right = chat("s2", "/tmp/a");
    const file = newFileTab("/tmp/a/src/lib.rs", "/tmp/a");
    const tab = {
      ...newTab("s1"),
      id: "t1",
      layout: {
        type: "split" as const,
        id: "split1",
        dir: "right" as const,
        children: [leaf("s1"), leaf("e1")],
        sizes: [0.5, 0.5],
      },
      focusedId: "e1",
      editorPanes: [{ id: "e1", files: [file], activeFileId: file.id }],
    };
    const snapshot = collectWorkspaceSnapshot(
      [tab],
      [left, right],
      "t1",
      "/tmp/a",
      new Map(),
    );
    const loaded = new Map([
      [
        "s1",
        {
          ...left,
          blocks: [
            ...left.blocks,
            { id: "a1", role: "assistant" as const, text: "stored" },
          ],
        },
      ],
    ]);
    const workspace = hydrateWorkspaceSnapshot(snapshot, loaded);
    expect(workspace?.tabs).toHaveLength(1);
    expect(workspace?.tabs[0]?.layout).toEqual(tab.layout);
    expect(workspace?.tabs[0]?.editorPanes[0]?.files[0]?.path).toBe(
      "/tmp/a/src/lib.rs",
    );
    expect(
      workspace?.sessions.find((session) => session.id === "s1")?.blocks,
    ).toEqual(loaded.get("s1")?.blocks);
    expect(
      workspace?.sessions.find((session) => session.id === "s2")?.blocks,
    ).toEqual([]);
  });

  it("marks in-flight chats interrupted and adds a tab if they were parked", () => {
    const open = chat("s1", "/tmp/a");
    const parked = chat("s2", "/tmp/a");
    parked.busy = true;
    const snapshot = collectWorkspaceSnapshot(
      [{ ...newTab("s1"), id: "t1" }],
      [open, parked],
      "t1",
      "/tmp/a",
      new Map(),
    );
    const workspace = hydrateWorkspaceSnapshot(
      snapshot,
      new Map([
        ["s1", open],
        ["s2", parked],
      ]),
      new Set(["s2"]),
    );
    expect(workspace?.tabs).toHaveLength(2);
    const resumed = workspace?.sessions.find((session) => session.id === "s2");
    expect(resumed?.busy).toBe(false);
    expect(
      resumed?.blocks.some((block) => block.text === INTERRUPT_MESSAGE),
    ).toBe(true);
  });

  it("keeps terminal-only tabs", () => {
    const term = newTerminalFile("/tmp/a");
    const tab = {
      kind: "session" as const,
      id: "t1",
      layout: leaf("p1"),
      focusedId: "p1",
      editorPanes: [],
      terminalPanes: [{ id: "p1", files: [term], activeFileId: term.id }],
    };
    const snapshot = collectWorkspaceSnapshot(
      [tab],
      [],
      "t1",
      "/tmp/a",
      new Map(),
    );
    const workspace = hydrateWorkspaceSnapshot(snapshot, new Map());
    expect(workspace?.tabs[0]?.terminalPanes[0]?.files[0]?.terminal).toBe(true);
  });

  it("restores a project terminal dock", () => {
    const term = { ...newTerminalFile("/tmp/a"), foreground: "vite" };
    const dock = {
      ...createProjectTerminal("/tmp/a", term),
      side: "left" as const,
      size: 300,
      open: false,
    };
    const snapshot = collectWorkspaceSnapshot(
      [{ ...newTab("s1"), id: "t1" }],
      [],
      "t1",
      "/tmp/a",
      new Map(),
      [dock],
    );
    const workspace = hydrateWorkspaceSnapshot(snapshot, new Map());
    expect(workspace?.projectTerminals).toEqual([
      expect.objectContaining({
        projectPath: "/tmp/a",
        side: "left",
        size: 300,
        open: false,
      }),
    ]);
    expect(workspace?.projectTerminals?.[0]?.pane.files[0]?.terminal).toBe(
      true,
    );
    expect(
      workspace?.projectTerminals?.[0]?.pane.files[0]?.foreground,
    ).toBeUndefined();
  });
});
