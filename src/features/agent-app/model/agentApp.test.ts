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
    draft: vi.fn(async () => ({ alreadySaved: false, draft: true })),
    notes: vi.fn(async () => [note]),
    note: vi.fn(async (id) => (id === note.id ? note : null)),
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

  it("does not let app-supplied prompts enable /mono in another session", async () => {
    const { source, host } = fixture();
    for (const action of ["sessions.send", "sessions.draft", "sessions.start"]) {
      for (const prompt of ["/mono list notes", "  /MONOCODE list notes"]) {
        await expect(
          handleAgentApp(
            source,
            "blocked",
            action,
            action === "sessions.start" ? { prompt } : { sessionId: "other", prompt },
            host,
          ),
        ).rejects.toThrow("cannot enable /mono");
      }
    }
    expect(host.send).not.toHaveBeenCalled();
    expect(host.draft).not.toHaveBeenCalled();
    expect(host.start).not.toHaveBeenCalled();

    await handleAgentApp(
      source,
      "ordinary",
      "sessions.send",
      { sessionId: "other", prompt: "Explain the /mono command" },
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
});
