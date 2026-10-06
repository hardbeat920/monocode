// @vitest-environment happy-dom
import { beforeEach, afterEach, describe, expect, it, vi } from "vitest";
import { newSession } from "../../sessions/model/session";
import {
  resetHarnessModelOverlays,
  setHarnessModels,
} from "../../sessions/model/models";
import {
  loadSessionFolders,
  saveSessionFolders,
} from "../../sessions/model/sessionFolders";
import type { Note } from "../../notes";
import type { Worktree } from "../../source-control/model/worktrees";
import { handleAgentApp, notePreview, type AgentAppHost } from "./agentApp";

vi.mock("../../../integrations/harness/core/availability", () => ({
  isHarnessAvailable: (id: string) => id === "codex",
}));

const note: Note = {
  id: "n1",
  slug: "plan",
  title: "Plan",
  body: "First paragraph.\n\nSecond paragraph.\n\nThird paragraph should stay out of list.",
  tags: ["work"],
  createdAt: 1,
  updatedAt: 2,
};
const featureWorktree: Worktree = {
  path: "/tmp/project-worktrees/feature",
  branch: "feature",
  head: "abc123",
  isMain: false,
  locked: false,
  prunable: false,
  missing: false,
  dirty: false,
  unpushed: 0,
  sessionIds: [],
};

const storedValues = new Map<string, string>();
vi.stubGlobal("localStorage", {
  clear: () => storedValues.clear(),
  getItem: (key: string) => storedValues.get(key) ?? null,
  setItem: (key: string, value: string) => storedValues.set(key, value),
  removeItem: (key: string) => storedValues.delete(key),
});

beforeEach(() => {
  localStorage.clear();
  setHarnessModels("codex", [
    {
      id: "codex:test",
      harness: "codex",
      name: "Test model",
      settings: [
        {
          id: "effort",
          label: "Effort",
          kind: "select",
          value: "medium",
          options: [
            { value: "medium", label: "Medium" },
            { value: "high", label: "High" },
          ],
        },
      ],
    },
  ]);
});

afterEach(() => {
  resetHarnessModelOverlays();
});

/** Provide a project caller and idle target for dispatch tests. */
function fixture() {
  const source = newSession("codex", "/tmp/project", "codex:test");
  source.id = "lead";
  const host: AgentAppHost = {
    start: vi.fn(async () => {}),
    /** List the fixture's idle target in the caller's project. */
    sessions: vi.fn(async () => [
      {
        id: "other",
        title: "Other",
        harness: "codex",
        model: "codex:test",
        busy: false,
        hasDraft: false,
        archived: false,
      },
    ]),
    session: vi.fn(async (id) =>
      id === "other" ? { ...newSession("codex", source.cwd), id } : null,
    ),
    /** Model the fixture write as changed only for the archive request. */
    archive: vi.fn(async (_id, archived) => ({ changed: archived })),
    send: vi.fn(async () => ({ alreadySubmitted: false })),
    draft: vi.fn(async () => ({ alreadySaved: false, draft: true })),
    worktrees: vi.fn(async () => ({
      worktrees: [
        { ...featureWorktree },
        { ...featureWorktree, path: source.cwd, isMain: true, branch: "main" },
      ],
      defaultRoot: "/tmp/project-worktrees",
    })),
    createWorktree: vi.fn(async () => ({ ...featureWorktree })),
    notes: vi.fn(async () => [note]),
    note: vi.fn(async (id) => (id === note.id ? note : null)),
    saveNote: vi.fn(async (input) => ({ ...note, ...input })),
  };
  return { source, host };
}

describe("agent app commands", () => {
  it("reads a listed project session in bounded pages", async () => {
    const { source, host } = fixture();
    const result = await handleAgentApp(
      source,
      "read-1",
      "sessions.read",
      { sessionId: "other" },
      host,
    );
    expect(result).toMatchObject({ sessionId: "other", turns: [] });
    await expect(
      handleAgentApp(
        source,
        "read-2",
        "sessions.read",
        { sessionId: "missing" },
        host,
      ),
    ).rejects.toThrow("not found in this project");
  });

  it("sends a follow-up only to a listed idle session", async () => {
    const { source, host } = fixture();
    expect(
      await handleAgentApp(
        source,
        "send-1",
        "sessions.send",
        { sessionId: "other", prompt: "Continue the review" },
        host,
      ),
    ).toMatchObject({ sessionId: "other", submitted: true });
    expect(host.send).toHaveBeenCalledWith(
      "other",
      "Continue the review",
      "app-lead-send-1",
    );
    await expect(
      handleAgentApp(
        source,
        "send-2",
        "sessions.send",
        { sessionId: "lead", prompt: "loop" },
        host,
      ),
    ).rejects.toThrow("current conversation");
    await expect(
      handleAgentApp(
        source,
        "send-3",
        "sessions.send",
        { sessionId: "missing", prompt: "hello" },
        host,
      ),
    ).rejects.toThrow("not found in this project");
    expect(host.send).toHaveBeenCalledTimes(1);
  });

  /** Report transitions and refuse self or busy targets. */
  it("archives and restores only another idle session in the project", async () => {
    const { source, host } = fixture();
    expect(
      await handleAgentApp(
        source,
        "archive-1",
        "sessions.archive",
        { sessionId: "other" },
        host,
      ),
    ).toEqual({ sessionId: "other", archived: true, changed: true });
    expect(host.archive).toHaveBeenCalledWith("other", true);
    expect(
      await handleAgentApp(
        source,
        "restore-1",
        "sessions.unarchive",
        { sessionId: "other" },
        host,
      ),
    ).toEqual({ sessionId: "other", archived: false, changed: false });
    expect(host.archive).toHaveBeenLastCalledWith("other", false);
    await expect(
      handleAgentApp(
        source,
        "self",
        "sessions.archive",
        { sessionId: source.id },
        host,
      ),
    ).rejects.toThrow("current session");
    await expect(
      handleAgentApp(
        source,
        "busy",
        "sessions.archive",
        { sessionId: "other" },
        {
          ...host,
          session: vi.fn(async () => ({
            ...newSession("codex", source.cwd),
            id: "other",
            busy: true,
          })),
        },
      ),
    ).rejects.toThrow("busy");
    expect(host.archive).toHaveBeenCalledTimes(2);
  });

  /** Require the requested ID to belong to the caller's project. */
  it("does not archive missing or cross-project sessions", async () => {
    const { source, host } = fixture();
    await expect(
      handleAgentApp(
        source,
        "missing",
        "sessions.archive",
        { sessionId: "missing" },
        host,
      ),
    ).rejects.toThrow("not found in this project");
    host.session = vi.fn(async (id) =>
      id === "other"
        ? { ...newSession("codex", "/tmp/another-project"), id }
        : null,
    );
    await expect(
      handleAgentApp(
        source,
        "cross-project",
        "sessions.archive",
        { sessionId: "other" },
        host,
      ),
    ).rejects.toThrow("not found in this project");
    expect(host.archive).not.toHaveBeenCalled();
  });

  /** Keep archived sessions discoverable with draft state intact. */
  it("lists archived state without hiding archived sessions", async () => {
    const { source, host } = fixture();
    host.sessions = vi.fn(async () => [
      {
        id: "other",
        title: "Other",
        harness: "codex",
        model: "codex:test",
        busy: false,
        hasDraft: true,
        archived: true,
      },
    ]);
    expect(
      await handleAgentApp(source, "list", "sessions.list", {}, host),
    ).toMatchObject({
      sessions: [{ id: "other", archived: true, hasDraft: true }],
    });
  });

  it("saves an unsent draft in another listed project session", async () => {
    const { source, host } = fixture();
    expect(
      await handleAgentApp(
        source,
        "draft-1",
        "sessions.draft",
        { sessionId: "other", prompt: "Review this idea later" },
        host,
      ),
    ).toMatchObject({ sessionId: "other", saved: true, draft: true });
    expect(host.draft).toHaveBeenCalledWith(
      "other",
      "Review this idea later",
      "app-lead-draft-1",
    );
    await expect(
      handleAgentApp(
        source,
        "draft-2",
        "sessions.draft",
        { sessionId: "lead", prompt: "Not here" },
        host,
      ),
    ).rejects.toThrow("composer");
    await expect(
      handleAgentApp(
        source,
        "draft-3",
        "sessions.draft",
        { sessionId: "missing", prompt: "Not there" },
        host,
      ),
    ).rejects.toThrow("not found in this project");
    expect(host.draft).toHaveBeenCalledTimes(1);
  });

  it("does not let app-supplied prompts enable Operator in another session", async () => {
    const { source, host } = fixture();
    for (const action of [
      "sessions.send",
      "sessions.draft",
      "sessions.start",
    ]) {
      for (const prompt of [
        "/operator list notes",
        "/mono list notes",
        "  /MONOCODE list notes",
      ]) {
        await expect(
          handleAgentApp(
            source,
            "blocked",
            action,
            action === "sessions.start"
              ? { prompt }
              : { sessionId: "other", prompt },
            host,
          ),
        ).rejects.toThrow("cannot enable /operator");
      }
    }
    expect(host.send).not.toHaveBeenCalled();
    expect(host.draft).not.toHaveBeenCalled();
    expect(host.start).not.toHaveBeenCalled();

    await handleAgentApp(
      source,
      "ordinary",
      "sessions.send",
      { sessionId: "other", prompt: "Explain the /operator command" },
      host,
    );
    expect(host.send).toHaveBeenCalledOnce();
  });

  it("starts a submitted tab with explicit model, effort, permissions and workspace", async () => {
    const { source, host } = fixture();
    const result = await handleAgentApp(
      source,
      "request-1",
      "sessions.start",
      {
        prompt: "Inspect the API",
        model: "codex:test",
        effort: "high",
        runtimeMode: "auto-accept-edits",
        workspaceMode: "worktree",
        reveal: true,
      },
      host,
    );
    expect(host.start).toHaveBeenCalledWith(
      expect.objectContaining({
        prompt: "Inspect the API",
        cwd: "/tmp/project",
        model: "codex:test",
        modelSettings: { effort: "high" },
        runtimeMode: "auto-accept-edits",
        workspaceMode: "worktree",
        reveal: true,
      }),
      "app-lead-request-1",
    );
    expect(result).toMatchObject({ id: "app-lead-request-1", submitted: true });
  });

  it("lists project worktrees and starts on a selected existing checkout", async () => {
    const { source, host } = fixture();
    source.worktreeCwd = "/tmp/project-worktrees/other";
    const listed = await handleAgentApp(
      source,
      "list",
      "worktrees.list",
      {},
      host,
    );
    expect((listed as { worktrees: Worktree[] }).worktrees[0]).toMatchObject({
      branch: "feature",
    });
    expect(host.worktrees).toHaveBeenCalledWith(source.cwd);

    await handleAgentApp(
      source,
      "feature",
      "sessions.start",
      {
        prompt: "Review feature",
        worktreeCwd: featureWorktree.path,
      },
      host,
    );
    expect(host.start).toHaveBeenCalledWith(
      expect.objectContaining({ worktreeCwd: featureWorktree.path }),
      "app-lead-feature",
    );
    await handleAgentApp(
      source,
      "main",
      "sessions.start",
      {
        prompt: "Review main",
        worktreeCwd: source.cwd,
      },
      host,
    );
    expect(host.start).toHaveBeenLastCalledWith(
      expect.objectContaining({ worktreeCwd: undefined }),
      "app-lead-main",
    );
  });

  it("rejects unavailable or conflicting worktree choices before launching", async () => {
    const { source, host } = fixture();
    for (const input of [
      { prompt: "A", worktreeCwd: "/tmp/other-repo" },
      {
        prompt: "A",
        workspaceMode: "worktree",
        worktreeCwd: featureWorktree.path,
      },
      { prompt: "A", worktreeBase: "main", worktreeCwd: featureWorktree.path },
    ]) {
      await expect(
        handleAgentApp(source, "invalid", "sessions.start", input, host),
      ).rejects.toThrow();
    }
    vi.mocked(host.worktrees).mockResolvedValueOnce({
      worktrees: [{ ...featureWorktree, missing: true }],
      defaultRoot: "/tmp/project-worktrees",
    });
    await expect(
      handleAgentApp(
        source,
        "missing",
        "sessions.start",
        { prompt: "A", worktreeCwd: featureWorktree.path },
        host,
      ),
    ).rejects.toThrow("unavailable in this project");
    expect(host.start).not.toHaveBeenCalled();
  });

  it("creates a worktree on a named new or existing branch", async () => {
    const { source, host } = fixture();
    expect(
      await handleAgentApp(
        source,
        "new",
        "worktrees.create",
        {
          branch: "feature",
          base: "origin/main",
        },
        host,
      ),
    ).toMatchObject({ path: featureWorktree.path });
    expect(host.createWorktree).toHaveBeenCalledWith(
      source.cwd,
      "feature",
      "origin/main",
      false,
    );
    await handleAgentApp(
      source,
      "existing",
      "worktrees.create",
      {
        branch: "feature",
        existing: true,
      },
      host,
    );
    expect(host.createWorktree).toHaveBeenLastCalledWith(
      source.cwd,
      "feature",
      "HEAD",
      true,
    );
    await expect(
      handleAgentApp(
        source,
        "bad",
        "worktrees.create",
        {
          branch: "feature",
          base: "main",
          existing: true,
        },
        host,
      ),
    ).rejects.toThrow("base cannot be set");
    expect(host.createWorktree).toHaveBeenCalledTimes(2);
  });

  it("starts a pane beside the caller or another session in either direction", async () => {
    const { source, host } = fixture();
    await handleAgentApp(
      source,
      "right",
      "sessions.start",
      { prompt: "Inspect the API", placement: "right", draft: true },
      host,
    );
    expect(host.start).toHaveBeenLastCalledWith(
      expect.objectContaining({ draft: true }),
      "app-lead-right",
      { direction: "right", besideSessionId: "lead" },
    );
    await handleAgentApp(
      source,
      "down",
      "sessions.start",
      {
        prompt: "Review the UI",
        placement: "down",
        besideSessionId: "app-lead-right",
      },
      host,
    );
    expect(host.start).toHaveBeenLastCalledWith(
      expect.anything(),
      "app-lead-down",
      { direction: "down", besideSessionId: "app-lead-right" },
    );
  });

  it("rejects invalid pane placement before starting", async () => {
    const { source, host } = fixture();
    for (const input of [
      { prompt: "A", placement: "left" },
      { prompt: "A", besideSessionId: "other" },
      { prompt: "A", placement: "down", besideSessionId: 42 },
    ]) {
      await expect(
        handleAgentApp(source, "invalid", "sessions.start", input, host),
      ).rejects.toThrow();
    }
    expect(host.start).not.toHaveBeenCalled();
  });

  it("inherits the caller's permission mode unless start overrides it", async () => {
    const { source, host } = fixture();
    source.runtimeMode = "auto";
    await handleAgentApp(
      source,
      "inherited-mode",
      "sessions.start",
      { prompt: "Review this" },
      host,
    );
    expect(host.start).toHaveBeenLastCalledWith(
      expect.objectContaining({ runtimeMode: "auto" }),
      "app-lead-inherited-mode",
    );
    await handleAgentApp(
      source,
      "explicit-mode",
      "sessions.start",
      { prompt: "Review this", runtimeMode: "full-access" },
      host,
    );
    expect(host.start).toHaveBeenLastCalledWith(
      expect.objectContaining({ runtimeMode: "full-access" }),
      "app-lead-explicit-mode",
    );
  });

  it("starts with an unsent draft and can immediately move the new session into a folder", async () => {
    const { source, host } = fixture();
    const result = (await handleAgentApp(
      source,
      "draft-launch",
      "sessions.start",
      {
        prompt: "Test prompt",
        model: "codex:test",
        draft: true,
      },
      host,
    )) as { id: string; submitted: boolean; draft: boolean };
    expect(result).toMatchObject({
      id: "app-lead-draft-launch",
      submitted: false,
      draft: true,
    });
    expect(host.start).toHaveBeenCalledWith(
      expect.objectContaining({ prompt: "Test prompt", draft: true }),
      result.id,
    );
    host.sessions = vi.fn(async () => [
      {
        id: result.id,
        title: "Test prompt",
        harness: "codex",
        model: "codex:test",
        busy: false,
        hasDraft: true,
        archived: false,
      },
    ]);
    const moved = await handleAgentApp(
      source,
      "folder",
      "folders.move",
      { sessionId: result.id, newFolderName: "test" },
      host,
    );
    expect(moved).toMatchObject({ sessionId: result.id, folderName: "test" });
  });

  it("rejects invalid model settings before starting a session", async () => {
    const { source, host } = fixture();
    await expect(
      handleAgentApp(
        source,
        "bad",
        "sessions.start",
        {
          prompt: "Hello",
          effort: "ultra",
        },
        host,
      ),
    ).rejects.toThrow("Invalid model setting effort");
    expect(host.start).not.toHaveBeenCalled();
    await expect(
      handleAgentApp(
        source,
        "bad-draft",
        "sessions.start",
        { prompt: "Hello", draft: "true" },
        host,
      ),
    ).rejects.toThrow("draft must be a boolean");
    await expect(
      handleAgentApp(source, "unexpected", "toString", {}, host),
    ).rejects.toThrow("Unknown app action");
  });

  it("moves an existing project session into a sidebar folder", async () => {
    const { source, host } = fixture();
    saveSessionFolders(source.cwd, [
      {
        id: "folder-1",
        name: "Research",
        sessionIds: ["lead"],
        collapsed: false,
      },
    ]);
    expect(
      await handleAgentApp(
        source,
        "move",
        "folders.move",
        {
          sessionId: "other",
          folderId: "folder-1",
        },
        host,
      ),
    ).toMatchObject({ folderId: "folder-1", sessionId: "other" });
    expect(loadSessionFolders(source.cwd)[0]?.sessionIds).toEqual([
      "lead",
      "other",
    ]);
  });

  it("creates a folder during a move and rejects unknown sessions", async () => {
    const { source, host } = fixture();
    await expect(
      handleAgentApp(
        source,
        "bad-move",
        "folders.move",
        {
          sessionId: "missing",
          newFolderName: "Research",
        },
        host,
      ),
    ).rejects.toThrow("Session was not found");
    const moved = (await handleAgentApp(
      source,
      "new-folder",
      "folders.move",
      {
        sessionId: "other",
        newFolderName: "Research",
      },
      host,
    )) as { folderId: string; folderName: string };
    expect(moved.folderName).toBe("Research");
    expect(loadSessionFolders(source.cwd)[0]).toMatchObject({
      id: moved.folderId,
      sessionIds: ["other"],
    });
  });

  it("lists short note previews and reads one full note on request", async () => {
    const { source, host } = fixture();
    const listed = (await handleAgentApp(
      source,
      "list",
      "notes.list",
      {},
      host,
    )) as {
      notes: Array<{ preview: string; body?: string }>;
    };
    expect(listed.notes[0]?.preview).toBe(
      "First paragraph.\n\nSecond paragraph.",
    );
    expect(listed.notes[0]).not.toHaveProperty("body");
    expect(notePreview("A".repeat(500))).toHaveLength(400);
    expect(
      await handleAgentApp(source, "read", "notes.read", { id: "n1" }, host),
    ).toMatchObject({ body: note.body });
  });

  it("creates a note with source metadata and reuses the same request ID safely", async () => {
    const { source, host } = fixture();
    let created: Note | null = null;
    host.note = vi.fn(async (id) => (id === created?.id ? created : null));
    host.saveNote = vi.fn(async (input) => {
      created = { ...note, ...input };
      return created;
    });
    const input = { body: "# Work plan\n\nNext steps", tags: ["#Work"] };
    const saved = await handleAgentApp(
      source,
      "create-1",
      "notes.write",
      input,
      host,
    );
    expect(saved).toMatchObject({
      id: "app-lead-create-1",
      title: "Work plan",
      body: input.body,
      tags: ["work"],
      sourceSessionId: source.id,
      sourceCwd: source.cwd,
    });
    expect(
      await handleAgentApp(source, "create-1", "notes.write", input, host),
    ).toEqual(saved);
    expect(host.saveNote).toHaveBeenCalledTimes(1);
    await expect(
      handleAgentApp(
        source,
        "create-1",
        "notes.write",
        {
          body: "Different body",
        },
        host,
      ),
    ).rejects.toThrow("Request ID was already used");
  });

  it("edits only supplied note fields and refuses missing or malformed notes", async () => {
    const { source, host } = fixture();
    const changed = await handleAgentApp(
      source,
      "edit-1",
      "notes.write",
      {
        id: "n1",
        body: "Updated body",
      },
      host,
    );
    expect(changed).toMatchObject({
      id: "n1",
      title: note.title,
      body: "Updated body",
      tags: note.tags,
    });
    expect(host.saveNote).toHaveBeenCalledWith({
      id: "n1",
      title: note.title,
      body: "Updated body",
      tags: note.tags,
    });
    await expect(
      handleAgentApp(
        source,
        "edit-2",
        "notes.write",
        {
          id: "missing",
          body: "x",
        },
        host,
      ),
    ).rejects.toThrow("Note was not found");
    await expect(
      handleAgentApp(
        source,
        "edit-3",
        "notes.write",
        {
          id: "n1",
        },
        host,
      ),
    ).rejects.toThrow("Supply title, body or tags");
    await expect(
      handleAgentApp(
        source,
        "edit-4",
        "notes.write",
        {
          id: "n1",
          tags: "work",
        },
        host,
      ),
    ).rejects.toThrow("tags must be an array");
    await expect(
      handleAgentApp(
        source,
        "create-2",
        "notes.write",
        {
          title: "Empty",
        },
        host,
      ),
    ).rejects.toThrow("body is required");
  });
});
