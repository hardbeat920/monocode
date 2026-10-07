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
import type { Artifact } from "../../artifacts/artifacts";
import type { Worktree } from "../../source-control/model/worktrees";
import {
  canAccessAgentAppProject,
  handleAgentApp,
  limitedAppPrompt,
  notePreview,
  type AgentAppAccess,
  type AgentAppHost,
} from "./agentApp";

vi.mock("../../../integrations/harness/core/availability", () => ({
  isHarnessAvailable: (id: string) => id === "codex",
}));

const note: Note = {
  id: "n1",
  slug: "plan",
  title: "Plan",
  body: "First paragraph.\n\nSecond paragraph.\n\nThird paragraph should stay out of list.",
  tags: ["work"],
  slugPending: false,
  createdAt: 1,
  updatedAt: 2,
};
const artifact: Artifact = {
  id: "artifact-1",
  kind: "document",
  title: note.title,
  body: note.body,
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
        archived: false,
      },
    ]),
    session: vi.fn(async (id) =>
      id === "other" ? { ...newSession("codex", source.cwd), id } : null,
    ),
    send: vi.fn(async () => ({ alreadySubmitted: false })),
    draft: vi.fn(async () => ({ alreadySaved: false, draft: true })),
    stop: vi.fn(async () => {}),
    remove: vi.fn(async () => {}),
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
    isMono: () => false,
  };
  return { source, host };
}

describe("agent app commands", () => {
  it.each([undefined, true, false])(
    "uses the Mono's sidebar preference for both submitted and draft sessions: %s",
    async (showStartedSessionsInSidebar) => {
      const { source, host } = fixture();
      host.isMono = () => true;
      host.monoOf = () => ({
        id: "mono",
        projects: [source.cwd],
        showStartedSessionsInSidebar,
      });
      for (const draft of [false, true]) {
        await handleAgentApp(source, `launch-${draft}`, "sessions.start", {
          prompt: "Review the project",
          draft,
          notifyOnComplete: false,
        }, host);
        const launch = vi.mocked(host.start).mock.calls.at(-1)![0];
        expect(launch.sidebarHidden).toBe(
          showStartedSessionsInSidebar === false ? true : undefined,
        );
      }
    },
  );

  it("uses the owning Mono's preference when a habit starts a session", async () => {
    const { source, host } = fixture();
    host.isHabitRun = () => true;
    host.monoOf = () => ({
      id: "mono",
      projects: [source.cwd],
      showStartedSessionsInSidebar: false,
    });
    await handleAgentApp(source, "habit-launch", "sessions.start", {
      prompt: "Review the project",
      notifyOnComplete: false,
    }, host);
    expect(host.start).toHaveBeenCalledWith(
      expect.objectContaining({ sidebarHidden: true }),
      "app-lead-habit-launch",
    );
  });

  describe.each([
    ["sessions.stop", "stopped"],
    ["sessions.archive", "archived"],
    ["sessions.delete", "deleted"],
  ] as const)("%s", (action, resultField) => {
    it("manages a running session in the caller's project", async () => {
      const { source, host } = fixture();
      vi.mocked(host.session).mockResolvedValue({
        ...newSession("codex", source.cwd),
        id: "other",
        busy: true,
      });
      expect(
        await handleAgentApp(
          source,
          "manage",
          action,
          { sessionId: "other" },
          host,
        ),
      ).toEqual({ sessionId: "other", [resultField]: true });
      if (action === "sessions.stop") {
        expect(host.stop).toHaveBeenCalledExactlyOnceWith("other");
        expect(host.remove).not.toHaveBeenCalled();
      } else {
        expect(host.remove).toHaveBeenCalledExactlyOnceWith(
          "other",
          action === "sessions.archive" ? "archive" : "delete",
        );
        expect(host.stop).not.toHaveBeenCalled();
      }
    });

    it("lets a Mono manage sessions in a selected assigned project", async () => {
      const { source, host } = fixture();
      source.cwd = "/home/user";
      host.isMono = (id) => id === source.id;
      host.monoOf = () => ({
        id: "mono",
        projects: ["/code/app", "/code/site"],
      });
      vi.mocked(host.session).mockResolvedValue({
        ...newSession("codex", "/code/site"),
        id: "other",
      });
      expect(
        await handleAgentApp(
          source,
          "manage",
          action,
          {
            sessionId: "other",
            project: "site",
          },
          host,
        ),
      ).toEqual({ sessionId: "other", [resultField]: true });
      expect(host.sessions).toHaveBeenCalledWith("/code/site");
      await expect(
        handleAgentApp(
          source,
          "outside",
          action,
          {
            sessionId: "other",
            project: "/code/unassigned",
          },
          host,
        ),
      ).rejects.toThrow("Not one of your projects");
      await expect(
        handleAgentApp(
          source,
          "wrong-project",
          action,
          {
            sessionId: "other",
            project: "/code/app",
          },
          host,
        ),
      ).rejects.toThrow("not found in this project");
      expect(
        vi.mocked(host.stop).mock.calls.length +
          vi.mocked(host.remove).mock.calls.length,
      ).toBe(1);
    });

    it("rejects the caller, missing sessions and sessions outside the selected project", async () => {
      const { source, host } = fixture();
      await expect(
        handleAgentApp(source, "self", action, { sessionId: source.id }, host),
      ).rejects.toThrow("calling session");
      await expect(
        handleAgentApp(
          source,
          "missing",
          action,
          { sessionId: "missing" },
          host,
        ),
      ).rejects.toThrow("not found in this project");
      vi.mocked(host.session).mockResolvedValue({
        ...newSession("codex", "/another-project"),
        id: "other",
      });
      await expect(
        handleAgentApp(source, "outside", action, { sessionId: "other" }, host),
      ).rejects.toThrow("not found in this project");
      expect(host.stop).not.toHaveBeenCalled();
      expect(host.remove).not.toHaveBeenCalled();
    });

    it("does not manage Mono chats, habit runs or orchestration workers", async () => {
      const { source, host } = fixture();
      host.isMono = () => true;
      await expect(
        handleAgentApp(source, "mono", action, { sessionId: "other" }, host),
      ).rejects.toThrow("Only regular project sessions");
      host.isMono = () => false;
      host.isHabitRun = () => true;
      await expect(
        handleAgentApp(source, "habit", action, { sessionId: "other" }, host),
      ).rejects.toThrow("Only regular project sessions");
      host.isHabitRun = () => false;
      vi.mocked(host.session).mockResolvedValue({
        ...newSession("codex", source.cwd),
        id: "other",
        orchestrationLeadId: "orchestrator",
      });
      await expect(
        handleAgentApp(source, "worker", action, { sessionId: "other" }, host),
      ).rejects.toThrow("Only regular project sessions");
      expect(host.stop).not.toHaveBeenCalled();
      expect(host.remove).not.toHaveBeenCalled();
    });

    it("rejects invalid request IDs and unsupported fields before making changes", async () => {
      const { source, host } = fixture();
      await expect(
        handleAgentApp(source, "bad/id", action, { sessionId: "other" }, host),
      ).rejects.toThrow("Invalid request ID");
      await expect(
        handleAgentApp(
          source,
          "extra",
          action,
          {
            sessionId: "other",
            deleteWorktree: true,
          },
          host,
        ),
      ).rejects.toThrow("Unknown");
      expect(host.stop).not.toHaveBeenCalled();
      expect(host.remove).not.toHaveBeenCalled();
    });

    it("reports lifecycle failures instead of claiming success", async () => {
      const { source, host } = fixture();
      vi.mocked(host.stop).mockRejectedValue(new Error("operation failed"));
      vi.mocked(host.remove).mockRejectedValue(new Error("operation failed"));
      await expect(
        handleAgentApp(source, "failed", action, { sessionId: "other" }, host),
      ).rejects.toThrow("operation failed");
    });
  });

  it("allows session inspection in a Mono's projects when its chat lives at home", () => {
    const { source } = fixture();
    source.cwd = "/home/user";
    const projects = ["/code/app", "/code/site"];
    expect(canAccessAgentAppProject(source, "/code/app/", projects)).toBe(true);
    expect(canAccessAgentAppProject(source, "/code/site", projects)).toBe(true);
    expect(canAccessAgentAppProject(source, source.cwd, projects)).toBe(false);
    expect(canAccessAgentAppProject(source, "/code/other", projects)).toBe(
      false,
    );
    expect(canAccessAgentAppProject(source, "/code/app", [])).toBe(false);
    expect(canAccessAgentAppProject(source, source.cwd)).toBe(true);
    expect(canAccessAgentAppProject(source, "/code/app")).toBe(false);
  });

  it.each([undefined, true])("reports completion of Mono launches with notifyOnComplete=%s", async (notifyOnComplete) => {
    const { source, host } = fixture();
    host.isMono = (id) => id === source.id;
    expect(
      await handleAgentApp(
        source,
        "monitored",
        "sessions.start",
        {
          prompt: "Review the API",
          ...(notifyOnComplete === undefined ? {} : { notifyOnComplete }),
        },
        host,
      ),
    ).toMatchObject({
      id: "app-lead-monitored",
      submitted: true,
      notifyOnComplete: true,
    });
    expect(host.start).toHaveBeenLastCalledWith(
      expect.objectContaining({ prompt: "Review the API" }),
      "app-lead-monitored",
      undefined,
      "lead",
    );
    const optedOut = await handleAgentApp(
      source,
      "ordinary",
      "sessions.start",
      { prompt: "Review", notifyOnComplete: false },
      host,
    );
    expect(optedOut).not.toHaveProperty("notifyOnComplete");
    expect(host.start).toHaveBeenLastCalledWith(
      expect.anything(),
      "app-lead-ordinary",
    );
  });

  it("does not monitor a Mono's unsent draft by default", async () => {
    const { source, host } = fixture();
    host.isMono = (id) => id === source.id;
    const result = await handleAgentApp(
      source,
      "draft",
      "sessions.start",
      { prompt: "Review the API", draft: true },
      host,
    );
    expect(result).toMatchObject({ submitted: false, draft: true });
    expect(result).not.toHaveProperty("notifyOnComplete");
    expect(host.start).toHaveBeenCalledWith(
      expect.objectContaining({ draft: true }),
      "app-lead-draft",
    );
  });

  it.each([false, true])("leaves non-Mono launches unmonitored when habitRun=%s", async (habitRun) => {
    const { source, host } = fixture();
    host.isHabitRun = () => habitRun;
    const result = await handleAgentApp(
      source,
      "ordinary",
      "sessions.start",
      { prompt: "Review the API" },
      host,
    );
    expect(result).not.toHaveProperty("notifyOnComplete");
    expect(host.start).toHaveBeenCalledWith(
      expect.anything(),
      "app-lead-ordinary",
    );
  });

  it("reports completion of a sent follow-up to its calling Mono", async () => {
    const { source, host } = fixture();
    host.isMono = (id) => id === source.id;
    expect(
      await handleAgentApp(
        source,
        "monitored-send",
        "sessions.send",
        {
          sessionId: "other",
          prompt: "Fix the findings",
          notifyOnComplete: true,
        },
        host,
      ),
    ).toMatchObject({ submitted: true, notifyOnComplete: true });
    expect(host.send).toHaveBeenCalledWith(
      "other",
      "Fix the findings",
      "app-lead-monitored-send",
      "lead",
    );
  });

  it.each([
    { reveal: undefined },
    { reveal: true },
    { reveal: true, notifyOnComplete: true },
    { reveal: true, draft: true },
    { reveal: true, placement: "right", besideSessionId: "other" },
  ])("keeps Mono-launched sessions in the background for %j", async (options) => {
    const { source, host } = fixture();
    host.isMono = (id) => id === source.id;
    await handleAgentApp(
      source,
      "background",
      "sessions.start",
      { prompt: "Review the API", ...options },
      host,
    );
    expect(vi.mocked(host.start).mock.calls[0][0]).toMatchObject({
      prompt: "Review the API",
      reveal: false,
    });
  });

  it("reports completion by default through split placement", async () => {
    const { source, host } = fixture();
    host.isMono = (id) => id === source.id;
    await handleAgentApp(
      source,
      "monitored-split",
      "sessions.start",
      {
        prompt: "Review",
        placement: "right",
      },
      host,
    );
    expect(host.start).toHaveBeenCalledWith(
      expect.anything(),
      "app-lead-monitored-split",
      {
        direction: "right",
        besideSessionId: "lead",
      },
      "lead",
    );
  });

  it.each(["sessions.start", "sessions.send"])(
    "validates completion notification options for %s before submission",
    async (action) => {
      const { source, host } = fixture();
      const input = {
        prompt: "Review",
        ...(action === "sessions.send" ? { sessionId: "other" } : {}),
      };
      await expect(
        handleAgentApp(
          source,
          "wrong-type",
          action,
          { ...input, notifyOnComplete: "yes" },
          host,
        ),
      ).rejects.toThrow("must be a boolean");
      await expect(
        handleAgentApp(
          source,
          "not-mono",
          action,
          { ...input, notifyOnComplete: true },
          host,
        ),
      ).rejects.toThrow("only available in a Mono");
      expect(host.start).not.toHaveBeenCalled();
      expect(host.send).not.toHaveBeenCalled();
      await handleAgentApp(
        source,
        "disabled",
        action,
        { ...input, notifyOnComplete: false },
        host,
      );
    },
  );

  it("rejects completion reports for an unsent draft", async () => {
    const { source, host } = fixture();
    host.isMono = () => true;
    await expect(
      handleAgentApp(
        source,
        "draft-notify",
        "sessions.start",
        {
          prompt: "Review",
          draft: true,
          notifyOnComplete: true,
        },
        host,
      ),
    ).rejects.toThrow("unsent draft");
    expect(host.start).not.toHaveBeenCalled();
  });

  it("lets a Mono page its own chat without requiring a project", async () => {
    const { source, host } = fixture();
    host.isMono = (id) => id === source.id;
    host.readConversation = vi.fn(async () => ({ sessionId: source.id, title: "Mono", busy: false, hasDraft: false, turns: [], nextBefore: "older" }));
    const options = { before: "cursor", limit: 3, maxChars: 1200 };
    expect(await handleAgentApp(source, "own-chat", "sessions.read", { sessionId: source.id, ...options }, host)).toMatchObject({ nextBefore: "older" });
    expect(host.readConversation).toHaveBeenCalledWith(source, options);
    expect(host.sessions).not.toHaveBeenCalled();
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

  it.each([
    ["/tmp/project", "/tmp/project-worktrees/source"],
    ["/tmp/other", undefined],
  ])("inherits a worktree only when launching in the source project: %s", async (project, expectedWorktree) => {
    const { source, host } = fixture();
    source.worktreeCwd = "/tmp/project-worktrees/source";
    host.isMono = () => true;
    host.monoOf = () => ({ id: "mono", projects: [source.cwd, "/tmp/other"] });
    await handleAgentApp(source, "launch", "sessions.start", {
      prompt: "Review the project",
      project,
      notifyOnComplete: false,
    }, host);
    const launch = vi.mocked(host.start).mock.calls[0][0];
    expect(launch.cwd).toBe(project);
    expect(launch.worktreeCwd).toBe(expectedWorktree);
  });

  it("validates an explicit worktree in the Mono's selected project", async () => {
    const { source, host } = fixture();
    source.worktreeCwd = "/tmp/project-worktrees/source";
    host.isMono = () => true;
    host.monoOf = () => ({ id: "mono", projects: [source.cwd, "/tmp/other"] });
    const chosen = "/tmp/other-worktrees/feature";
    vi.mocked(host.worktrees).mockResolvedValue({
      worktrees: [{ ...featureWorktree, path: chosen }],
      defaultRoot: "/tmp/other-worktrees",
    });
    await handleAgentApp(source, "launch", "sessions.start", {
      prompt: "Review the feature",
      project: "/tmp/other",
      worktreeCwd: chosen,
      notifyOnComplete: false,
    }, host);
    expect(host.worktrees).toHaveBeenCalledWith("/tmp/other");
    expect(host.start).toHaveBeenCalledWith(
      expect.objectContaining({ cwd: "/tmp/other", worktreeCwd: chosen }),
      "app-lead-launch",
    );
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

  it("saves artifacts separately, returns metadata, and attaches one reference on retries", async () => {
    const { source, host } = fixture();
    host.isMono = () => true;
    let created: Artifact | null = null;
    host.artifact = vi.fn(async (id) => (id === created?.id ? created : null));
    host.saveArtifact = vi.fn(async (input) => {
      created = { ...artifact, ...input };
      return created;
    });
    host.postArtifact = vi.fn();
    const input = {
      kind: "document",
      title: "PR review",
      body: "# PR review\n\nFull report",
      summary: "Merge queue and blockers",
    };
    const result = await handleAgentApp(
      source,
      "report-1",
      "artifacts.write",
      input,
      host,
    );
    expect(result).toEqual({
      id: "artifact-lead-report-1",
      kind: "document",
      title: input.title,
      summary: input.summary,
      saved: true,
      attached: true,
    });
    expect(result).not.toHaveProperty("body");
    expect(host.saveNote).not.toHaveBeenCalled();
    expect(host.saveArtifact).toHaveBeenCalledWith(
      expect.objectContaining({
        sourceSessionId: source.id,
        body: input.body,
        kind: "document",
      }),
    );
    expect(
      await handleAgentApp(source, "report-1", "artifacts.write", input, host),
    ).toEqual(result);
    expect(host.saveArtifact).toHaveBeenCalledTimes(1);
    await expect(
      handleAgentApp(
        source,
        "report-1",
        "artifacts.write",
        { ...input, body: "Different" },
        host,
      ),
    ).rejects.toThrow("Request ID was already used");
    expect(host.postArtifact).toHaveBeenCalledWith(source.id, {
      id: "artifact-lead-report-1",
      kind: "document",
      title: input.title,
      summary: input.summary,
    });
  });

  it("revises a document without changing omitted fields and refuses note IDs", async () => {
    const { source, host } = fixture();
    host.isMono = () => true;
    host.artifact = vi.fn(async (id) =>
      id === "artifact-1" ? { ...artifact, id } : null,
    );
    host.saveArtifact = vi.fn(async (input) => ({ ...artifact, ...input }));
    host.postArtifact = vi.fn();
    await handleAgentApp(
      source,
      "edit",
      "artifacts.write",
      { id: "artifact-1", body: "Revised" },
      host,
    );
    expect(host.saveArtifact).toHaveBeenCalledWith({
      id: "artifact-1",
      title: note.title,
      body: "Revised",
      kind: "document",
    });
    await expect(
      handleAgentApp(
        source,
        "bad",
        "artifacts.write",
        { id: note.id, body: "Oops" },
        host,
      ),
    ).rejects.toThrow("Artifact was not found");
    expect(host.saveNote).not.toHaveBeenCalled();
  });

  it("allows habit artifacts and recovers an attachment failure without saving twice", async () => {
    const { source, host } = fixture();
    host.isHabitRun = () => true;
    let created: Artifact | null = null;
    host.artifact = vi.fn(async () => created);
    host.saveArtifact = vi.fn(async (input) => {
      created = { ...artifact, ...input };
      return created;
    });
    host.postArtifact = vi
      .fn()
      .mockRejectedValueOnce(new Error("Attachment failed"))
      .mockResolvedValue(undefined);
    const input = { body: "# Habit report\n\nDetails" };
    await expect(
      handleAgentApp(source, "report", "artifacts.write", input, host),
    ).rejects.toThrow("Attachment failed");
    await expect(
      handleAgentApp(source, "report", "artifacts.write", input, host),
    ).resolves.toMatchObject({ attached: true });
    expect(host.saveArtifact).toHaveBeenCalledTimes(1);
  });

  it("restricts artifact actions to Monos and lists only document metadata", async () => {
    const { source, host } = fixture();
    await expect(
      handleAgentApp(
        source,
        "report",
        "artifacts.write",
        { body: "Report" },
        host,
      ),
    ).rejects.toThrow("Only a Mono");
    host.isMono = () => true;
    host.artifact = vi.fn(async () => artifact);
    host.artifacts = vi.fn(async () => [{ ...artifact, id: "artifact-1" }]);
    const result = await handleAgentApp(
      source,
      "list",
      "artifacts.list",
      {},
      host,
    );
    expect(result).toEqual({
      total: 1,
      offset: 0,
      artifacts: [
        {
          id: "artifact-1",
          kind: "document",
          title: artifact.title,
          updatedAt: 2,
        },
      ],
    });
    expect(host.notes).not.toHaveBeenCalled();
    await expect(
      handleAgentApp(
        source,
        "list-bad",
        "artifacts.list",
        { limit: 101 },
        host,
      ),
    ).rejects.toThrow("limit");
    await expect(
      handleAgentApp(
        source,
        "read",
        "artifacts.read",
        { id: "artifact-1" },
        host,
      ),
    ).resolves.toMatchObject({ body: note.body });
  });

  it("rejects unsupported artifact kinds before saving or posting a card", async () => {
    const { source, host } = fixture();
    host.isMono = () => true;
    host.artifact = vi.fn(async () => null);
    host.artifacts = vi.fn(async () => []);
    host.saveArtifact = vi.fn();
    host.postArtifact = vi.fn();
    for (const kind of ["code", "preview", "note", null]) {
      await expect(
        handleAgentApp(
          source,
          "bad",
          "artifacts.write",
          {
            kind,
            body: "Content",
          },
          host,
        ),
      ).rejects.toThrow("Unsupported artifact kind");
    }
    await expect(
      handleAgentApp(
        source,
        "bad",
        "artifacts.list",
        {
          kind: "code",
        },
        host,
      ),
    ).rejects.toThrow("Unsupported artifact kind");
    expect(host.saveArtifact).not.toHaveBeenCalled();
    expect(host.postArtifact).not.toHaveBeenCalled();
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

describe("agent app access without /operator", () => {
  const limited: AgentAppAccess = { scope: "limited", openSessions: true };

  it("allows listing and starting sessions but nothing else", async () => {
    const { source, host } = fixture();
    await expect(
      handleAgentApp(source, "l1", "sessions.list", {}, host, limited),
    ).resolves.toMatchObject({ cwd: source.cwd });
    for (const [action, input] of [
      ["sessions.read", { sessionId: "other" }],
      ["sessions.send", { sessionId: "other", prompt: "Go" }],
      ["sessions.draft", { sessionId: "other", prompt: "Go" }],
      ["notes.list", {}],
      ["folders.list", {}],
    ] as const) {
      await expect(
        handleAgentApp(source, "l2", action, input, host, limited),
      ).rejects.toThrow("/operator");
    }
    expect(host.send).not.toHaveBeenCalled();
  });

  it("refuses to open sessions when the setting is off", async () => {
    const { source, host } = fixture();
    const off = { ...limited, openSessions: false };
    for (const action of ["sessions.list", "sessions.start"]) {
      await expect(
        handleAgentApp(source, "off", action, action === "sessions.start" ? { prompt: "Go" } : {}, host, off),
      ).rejects.toThrow("Let agents open sessions");
    }
    expect(host.start).not.toHaveBeenCalled();
  });

  it("submits a started session unless review is on, and honors the agent's draft", async () => {
    const { source, host } = fixture();
    expect(
      await handleAgentApp(source, "run", "sessions.start", { prompt: "Go" }, host, limited),
    ).toMatchObject({ submitted: true, draft: false });
    expect(vi.mocked(host.start).mock.calls[0][0].draft).toBeUndefined();
    expect(
      await handleAgentApp(source, "own-draft", "sessions.start", { prompt: "Go", draft: true }, host, limited),
    ).toMatchObject({ submitted: false, draft: true });
    const reviewed = await handleAgentApp(
      source,
      "review",
      "sessions.start",
      { prompt: "Go", draft: false },
      host,
      { ...limited, reviewOpenedSessions: true },
    );
    expect(reviewed).toMatchObject({ submitted: false, draft: true });
    expect(reviewed).toHaveProperty("note");
    expect(vi.mocked(host.start).mock.calls[2][0].draft).toBe(true);
  });

  it("keeps full /operator starts unchanged by the review setting", async () => {
    const { source, host } = fixture();
    expect(
      await handleAgentApp(source, "full", "sessions.start", { prompt: "Go" }, host, {
        scope: "full",
        openSessions: false,
        reviewOpenedSessions: true,
      }),
    ).toMatchObject({ submitted: true, draft: false });
  });

  it("reads and messages only linked peers", async () => {
    const { source, host } = fixture();
    const peer = { ...newSession("codex", "/elsewhere"), id: "peer", title: "Peer" };
    host.links = vi.fn(async () => [{ id: "peer", title: "Peer" }]);
    host.linkedSession = vi.fn(async (id) => (id === "peer" ? peer : null));
    host.sendLinked = vi.fn(async () => ({ queued: true, alreadySent: false }));
    const off = { ...limited, openSessions: false };
    await expect(
      handleAgentApp(source, "r", "links.read", { sessionId: "peer" }, host, off),
    ).resolves.toMatchObject({ sessionId: "peer", title: "Peer" });
    await expect(
      handleAgentApp(source, "s1", "links.send", { sessionId: "peer", message: "Done with auth" }, host, off),
    ).resolves.toEqual({ sessionId: "peer", queued: true, alreadySent: false });
    expect(host.sendLinked).toHaveBeenCalledWith(source, "peer", "Done with auth", "link-lead-s1");
    await expect(
      handleAgentApp(source, "s2", "links.send", { sessionId: "other", message: "Hi" }, host, off),
    ).rejects.toThrow("not linked");
    await expect(
      handleAgentApp(source, "s3", "links.read", { sessionId: source.id }, host, off),
    ).rejects.toThrow();
    await expect(
      handleAgentApp(source, "s4", "links.send", { sessionId: "peer", message: "/operator go" }, host, off),
    ).rejects.toThrow("/operator");
  });

  it("describes only what the thread can do", () => {
    expect(limitedAppPrompt({ cli: "mc app", openSessions: false, peers: [] })).toBe("");
    const open = limitedAppPrompt({ cli: "mc app", openSessions: true, peers: [] });
    expect(open).toContain("sessions.start");
    expect(open).toContain("starts working right away");
    expect(open).not.toContain("links.send");
    expect(
      limitedAppPrompt({ cli: "mc app", openSessions: true, reviewOpenedSessions: true, peers: [] }),
    ).toContain("unsent draft");
    const linked = limitedAppPrompt({
      cli: "mc app",
      openSessions: false,
      peers: [{ id: "peer", title: "Auth work" }],
    });
    expect(linked).toContain("Auth work (peer)");
    expect(linked).toContain("links.send");
    expect(linked).not.toContain("sessions.start");
  });
});
