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

function fixture() {
  const source = newSession("codex", "/tmp/project", "codex:test");
  source.id = "lead";
  const host: AgentAppHost = {
    start: vi.fn(async () => {}),
    sessions: vi.fn(async () => [
      {
        id: "other",
        title: "Other",
        harness: "codex",
        model: "codex:test",
        busy: false,
        hasDraft: false,
      },
    ]),
    session: vi.fn(async (id) =>
      id === "other" ? { ...newSession("codex", source.cwd), id } : null,
    ),
    send: vi.fn(async () => ({ alreadySubmitted: false })),
    stop: vi.fn(async () => true),
    tracked: vi.fn(() => []),
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
  it("exposes compact capabilities while preserving the old no-argument models.list shape", async () => {
    const { source, host } = fixture();
    const legacy = await handleAgentApp(source, "models", "models.list", {}, host) as { harnesses: { id: string; available: boolean; models: unknown[] }[] };
    const codex = legacy.harnesses.find((entry) => entry.id === "codex")!;
    expect(Object.keys(codex).sort()).toEqual(["available", "id", "models"]);
    expect(codex.models).toEqual([{ id: "codex:test", name: "Test model", settings: [expect.objectContaining({ id: "effort" })] }]);
    const compact = await handleAgentApp(source, "cap", "capabilities", {}, host) as { harnesses: { models?: unknown[] }[]; recentModels: unknown[] };
    expect(compact.harnesses.every((entry) => !("models" in entry))).toBe(true);
    expect(compact.recentModels).toEqual([]);
    await expect(handleAgentApp(source, "bad", "capabilities", { other: true }, host)).rejects.toThrow("Unknown capabilities fields");
    await expect(handleAgentApp(source, "bad", "models.list", { harness: "codex" }, host)).rejects.toThrow("Unknown models.list fields");
  });
  it("requires an exact launch ID even when discovery accepts partial model searches", async () => {
    const { source, host } = fixture();
    const result = await handleAgentApp(source, "find", "capabilities", { model: "TEST" }, host) as { harnesses: { id: string; models: { id: string }[] }[] };
    expect(result.harnesses.find((entry) => entry.id === "codex")?.models[0].id).toBe("codex:test");
    await expect(handleAgentApp(source, "partial", "sessions.start", { prompt: "Task", model: "test" }, host)).rejects.toThrow("exact model IDs");
    expect(host.start).not.toHaveBeenCalled();
    await handleAgentApp(source, "exact", "sessions.start", { prompt: "Task", model: "codex:test" }, host);
    expect(host.start).toHaveBeenCalledOnce();
  });
  it("validates optional display names and reports post-acceptance persistence errors as accepted", async () => {
    const { source, host } = fixture();
    const start = vi.fn(async () => ({ persistenceError: "Work was accepted; disk full" }));
    host.start = start;
    const result = await handleAgentApp(source, "named", "sessions.start", { prompt: "Task", name: "  Reviewer  " }, host);
    expect(start).toHaveBeenCalledWith(expect.objectContaining({ prompt: "Task" }), "app-lead-named", undefined, "lead", { name: "Reviewer" });
    expect(result).toMatchObject({ submitted: true, persistenceError: "Work was accepted; disk full" });
    for (const name of ["", 1, "x".repeat(81)]) {
      await expect(handleAgentApp(source, "invalid", "sessions.start", { prompt: "Task", name }, host)).rejects.toThrow("name must be");
    }
    expect(start).toHaveBeenCalledTimes(1);
  });
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

  it("stops another session through the host with the caller as parent", async () => {
    const { source, host } = fixture();
    await expect(
      handleAgentApp(
        source,
        "stop-self",
        "sessions.stop",
        { sessionId: "lead" },
        host,
      ),
    ).rejects.toThrow("Use the Stop button to stop the current session");
    expect(host.stop).not.toHaveBeenCalled();

    expect(
      await handleAgentApp(
        source,
        "stop-1",
        "sessions.stop",
        { sessionId: "other" },
        host,
      ),
    ).toEqual({ sessionId: "other", stopped: true });
    expect(host.stop).toHaveBeenCalledWith("lead", "other");

    host.stop = vi.fn(async () => false);
    expect(
      await handleAgentApp(
        source,
        "stop-2",
        "sessions.stop",
        { sessionId: "other" },
        host,
      ),
    ).toEqual({ sessionId: "other", stopped: false });
  });

  it("lists tracking only for sessions the caller started or messaged", async () => {
    const { source, host } = fixture();
    host.sessions = vi.fn(async () => [
      {
        id: "other",
        title: "Other",
        harness: "codex" as const,
        model: "codex:test",
        busy: true,
        hasDraft: false,
      },
      {
        id: "unrelated",
        title: "Unrelated",
        harness: "codex" as const,
        model: "codex:test",
        busy: false,
        hasDraft: false,
      },
    ]);
    host.tracked = vi.fn(() => [
      {
        childId: "other",
        generation: 2,
        status: "running" as const,
        held: false,
        pending: 1,
      },
    ]);
    const result = (await handleAgentApp(
      source,
      "list",
      "sessions.list",
      {},
      host,
    )) as { sessions: { id: string; tracked?: unknown }[] };
    expect(host.tracked).toHaveBeenCalledWith("lead");
    expect(result.sessions[0].tracked).toEqual({
      generation: 2,
      status: "running",
      held: false,
      pending: 1,
    });
    expect(result.sessions[1]).not.toHaveProperty("tracked");
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
      "lead",
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
      undefined,
      "lead",
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
      undefined,
      "lead",
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
      undefined,
      "lead",
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
      "lead",
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
      "lead",
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
      undefined,
      "lead",
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
      undefined,
      "lead",
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
      undefined,
      "lead",
    );
    host.sessions = vi.fn(async () => [
      {
        id: result.id,
        title: "Test prompt",
        harness: "codex",
        model: "codex:test",
        busy: false,
        hasDraft: true,
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
