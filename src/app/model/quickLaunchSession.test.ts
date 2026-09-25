// @vitest-environment happy-dom
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { rememberProject } from "../../features/projects/model/recents";
import { ProjectNotFoundError } from "../../features/projects/model/projectLocationError";
import { launchReceiver } from "../../features/quick-composer/model/launchDelivery";
import type { QuickLaunch } from "../../features/quick-composer/model/quickComposer";
import {
  newSession,
  type Attachment,
  type Session,
} from "../../features/sessions/model/session";
import {
  newTab,
  type WorkspaceTab,
} from "../../features/workspace/model/layout";
import { filterTabsForProject } from "../../features/workspace/model/workspaceTabGroups";
import { acceptQuickLaunch } from "./quickLaunchSession";
import {
  submitAfterProjectSync,
  type SubmissionAcceptance,
} from "./submissionAcceptance";

const storedValues = new Map<string, string>();
vi.stubGlobal("localStorage", {
  clear: () => storedValues.clear(),
  getItem: (key: string) => storedValues.get(key) ?? null,
  setItem: (key: string, value: string) => storedValues.set(key, value),
  removeItem: (key: string) => storedValues.delete(key),
});

beforeEach(() => {
  localStorage.clear();
  vi.useFakeTimers();
});
afterEach(() => {
  vi.clearAllTimers();
  vi.useRealTimers();
});

function setup(reveal = false) {
  const oldSession = newSession("codex", "/old-project");
  const oldTab = newTab(oldSession.id);
  const state = {
    sessions: [oldSession],
    tabs: [oldTab],
    projectCwd: oldSession.cwd,
    recents: rememberProject(oldSession.cwd),
    activeTabId: oldTab.id,
  };
  const request: QuickLaunch = {
    cwd: "/new-project",
    harness: "codex",
    prompt: "hello from the floating composer",
    reveal,
  };
  const commit = vi.fn(
    (id: string, text: string, _attachments: Attachment[]) => {
      state.sessions = state.sessions.map((session) =>
        session.id === id
          ? {
              ...session,
              blocks: [
                ...session.blocks,
                { id: "user-turn", role: "user" as const, text },
              ],
            }
          : session,
      );
      return true;
    },
  );
  const submit = vi.fn(
    (...args: Parameters<typeof commit>): SubmissionAcceptance =>
      commit(...args),
  );
  const workspace = {
    getSessions: () => state.sessions,
    updateSessions: (update: (sessions: Session[]) => Session[]) => {
      state.sessions = update(state.sessions);
    },
    appendTab: vi.fn((tab: WorkspaceTab) => {
      state.tabs.push(tab);
    }),
    setProjectCwd: vi.fn((cwd: string) => {
      state.projectCwd = cwd;
    }),
    setRecents: vi.fn((recents: typeof state.recents) => {
      state.recents = recents;
    }),
    revealTab: vi.fn((id: string, cwd: string) => {
      // Assert the title bar and recents already point at the revealed project.
      expect(
        filterTabsForProject(state.tabs, state.sessions, state.projectCwd).some(
          (tab) => tab.id === id,
        ),
      ).toBe(true);
      expect(state.recents[0].path).toBe(request.cwd);
      expect(cwd).toBe(request.cwd);
      state.activeTabId = id;
    }),
    submit,
    saveDraft: vi.fn(
      (
        id: string,
        text: string,
        _attachments: Attachment[],
        requestId: string,
      ) => {
        state.sessions = state.sessions.map((session) =>
          session.id === id
            ? {
                ...session,
                blocks: [
                  ...session.blocks,
                  {
                    id: "draft-turn",
                    role: "user" as const,
                    text,
                    draft: true,
                    appRequestId: requestId,
                  },
                ],
              }
            : session,
        );
        return true;
      },
    ),
  };
  const queue = [{ id: "quick-session", request }];
  const ack = vi.fn(async () => {
    queue.shift();
  });
  const receive = launchReceiver({
    take: async () => queue[0] ?? null,
    accept: (launch, id) => acceptQuickLaunch(launch, id, workspace),
    ack,
    accepted: new Set(),
    accepting: new Map(),
    disposed: () => false,
  });
  return { state, oldTab, request, commit, workspace, queue, ack, receive };
}

it("switches project and recents before revealing a cross-project quick session", async () => {
  const { state, workspace, receive } = setup(true);
  await receive();
  expect(state.projectCwd).toBe("/new-project");
  expect(state.recents.map((entry) => entry.path)).toEqual([
    "/new-project",
    "/old-project",
  ]);
  expect(workspace.revealTab).toHaveBeenCalledOnce();
  expect(
    filterTabsForProject(state.tabs, state.sessions, state.projectCwd).map(
      (tab) => tab.id,
    ),
  ).toContain(state.activeTabId);
});

it("leaves the selected project, recents, and active tab unchanged for background launches", async () => {
  const { state, oldTab, workspace, receive } = setup();
  await receive();
  expect(state.projectCwd).toBe("/old-project");
  expect(state.activeTabId).toBe(oldTab.id);
  expect(state.recents.map((entry) => entry.path)).toEqual(["/old-project"]);
  expect(workspace.setProjectCwd).not.toHaveBeenCalled();
  expect(workspace.setRecents).not.toHaveBeenCalled();
  expect(workspace.revealTab).not.toHaveBeenCalled();
});

it("creates a draft-only session without submitting an agent turn", async () => {
  const { state, request, workspace } = setup();
  request.draft = true;
  await acceptQuickLaunch(request, "quick-session", workspace);
  expect(workspace.submit).not.toHaveBeenCalled();
  expect(workspace.saveDraft).toHaveBeenCalledWith(
    "quick-session",
    request.prompt,
    [],
    "quick-session",
  );
  expect(
    state.sessions.find((session) => session.id === "quick-session"),
  ).toMatchObject({
    quickLaunchAccepted: true,
    blocks: [
      {
        role: "user",
        text: request.prompt,
        draft: true,
        appRequestId: "quick-session",
      },
    ],
  });
  await acceptQuickLaunch(request, "quick-session", workspace);
  expect(workspace.saveDraft).toHaveBeenCalledOnce();
  state.sessions = state.sessions.map((session) =>
    session.id === "quick-session"
      ? {
          ...session,
          quickLaunchAccepted: undefined,
          blocks: session.blocks.map((block) => ({ ...block, draft: false })),
        }
      : session,
  );
  await acceptQuickLaunch(request, "quick-session", workspace);
  expect(workspace.saveDraft).toHaveBeenCalledOnce();
});

it("does not acknowledge or mark accepted while project synchronization is pending", async () => {
  const { state, workspace, commit, ack, receive } = setup();
  let resolveSync!: (location: {
    path: string;
    identity: string;
    moved: boolean;
  }) => void;
  const sync = new Promise<{ path: string; identity: string; moved: boolean }>(
    (resolve) => {
      resolveSync = resolve;
    },
  );
  workspace.submit.mockImplementationOnce((...args) =>
    submitAfterProjectSync({
      cwd: "/new-project",
      sync,
      applyLocationChange: vi.fn(),
      submit: () => commit(...args),
      onError: vi.fn(),
    }),
  );
  const pending = receive();
  await vi.waitFor(() => expect(workspace.submit).toHaveBeenCalledOnce());
  expect(ack).not.toHaveBeenCalled();
  expect(commit).not.toHaveBeenCalled();
  expect(
    state.sessions.find((s) => s.id === "quick-session")?.quickLaunchAccepted,
  ).toBeUndefined();
  resolveSync({ path: "/new-project", identity: "repo", moved: false });
  await pending;
  expect(commit).toHaveBeenCalledOnce();
  expect(ack).toHaveBeenCalledOnce();
  expect(
    state.sessions.find((s) => s.id === "quick-session")?.quickLaunchAccepted,
  ).toBe(true);
});

it.each(["sync failure", "deferred rejection", "deferred exception"])(
  "retains the prompt after %s and retries the same session",
  async (failure) => {
    const { state, workspace, commit, queue, ack, receive } = setup();
    const onError = vi.fn();
    const deferredSubmit = vi.fn(() => {
      if (failure === "deferred exception")
        throw new Error("submission failed");
      return false;
    });
    workspace.submit.mockImplementationOnce(() =>
      submitAfterProjectSync({
        cwd: "/new-project",
        sync:
          failure === "sync failure"
            ? Promise.reject(new Error("disk unavailable"))
            : Promise.resolve({
                path: "/new-project",
                identity: "repo",
                moved: false,
              }),
        applyLocationChange: vi.fn(),
        submit: deferredSubmit,
        onError,
      }),
    );
    await expect(receive()).rejects.toThrow("could not accept");
    expect(ack).not.toHaveBeenCalled();
    expect(queue).toHaveLength(1);
    const session = state.sessions.find((s) => s.id === "quick-session")!;
    expect(session.quickLaunchAccepted).toBeUndefined();
    expect(session.blocks.some((block) => block.role === "user")).toBe(false);
    expect(deferredSubmit).toHaveBeenCalledTimes(
      failure.startsWith("deferred") ? 1 : 0,
    );
    expect(onError).toHaveBeenCalledTimes(
      failure === "deferred rejection" ? 0 : 1,
    );

    await vi.advanceTimersByTimeAsync(250);
    expect(queue).toHaveLength(0);
    expect(ack).toHaveBeenCalledOnce();
    expect(commit).toHaveBeenCalledOnce();
    expect(state.sessions.filter((s) => s.id === "quick-session")).toHaveLength(
      1,
    );
    expect(workspace.appendTab).toHaveBeenCalledOnce();
    expect(
      state.sessions.find((s) => s.id === "quick-session")?.quickLaunchAccepted,
    ).toBe(true);
  },
);

it("retains a missing project's prompt without automatic retries, then accepts an explicit retry after reconnection", async () => {
  const { state, workspace, commit, queue, ack, receive } = setup();
  const onError = vi.fn();
  let connected = false;
  workspace.submit.mockImplementation((...args) =>
    submitAfterProjectSync({
      cwd: "/new-project",
      sync: Promise.resolve(
        connected
          ? { path: "/new-project", identity: "repo", moved: false }
          : null,
      ),
      applyLocationChange: vi.fn(),
      submit: () => commit(...args),
      onError,
    }),
  );
  await expect(receive()).rejects.toBeInstanceOf(ProjectNotFoundError);
  await vi.advanceTimersByTimeAsync(300_000);
  expect(vi.getTimerCount()).toBe(0);
  expect(workspace.submit).toHaveBeenCalledOnce();
  expect(onError).toHaveBeenCalledOnce();
  expect(commit).not.toHaveBeenCalled();
  expect(ack).not.toHaveBeenCalled();
  expect(queue).toHaveLength(1);
  expect(
    state.sessions.find((s) => s.id === "quick-session")?.quickLaunchAccepted,
  ).toBeUndefined();

  connected = true;
  await receive();
  expect(queue).toHaveLength(0);
  expect(commit).toHaveBeenCalledOnce();
  expect(ack).toHaveBeenCalledOnce();
  expect(workspace.appendTab).toHaveBeenCalledOnce();
});

it("does not submit an accepted prompt again after a lost ACK", async () => {
  const { workspace, ack, receive, queue } = setup();
  ack.mockRejectedValueOnce(new Error("IPC interrupted"));
  await expect(receive()).rejects.toThrow("IPC interrupted");
  await vi.advanceTimersByTimeAsync(250);
  expect(workspace.submit).toHaveBeenCalledOnce();
  expect(queue).toHaveLength(0);
});
