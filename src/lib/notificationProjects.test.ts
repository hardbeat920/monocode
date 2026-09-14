// @vitest-environment happy-dom
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import {
  resolveNotificationProject,
  inboxNotificationProject,
  rememberNotificationProjects,
  knownNotificationProjectSelection,
  knownNotificationProject,
  refreshNotificationProjects,
  loadNotificationProjects,
} from "./notificationProjects";
import type { InboxItem } from "./githubTasks";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
beforeEach(() => {
  localStorage.clear();
  invoke.mockReset();
});
afterEach(() => vi.restoreAllMocks());

it("does not lose a Git change while an already queued refresh is running", async () => {
  let finishInitial!: (value: unknown) => void;
  let finishRefresh!: (value: unknown) => void;
  invoke.mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finishInitial = resolve;
      }),
  );
  invoke.mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finishRefresh = resolve;
      }),
  );
  const initial = resolveNotificationProject("C:/queued");
  const firstRefresh = refreshNotificationProjects(["C:/queued"]);
  finishInitial({ root: "C:/queued", commonDir: null, remote: null });
  await initial;
  await vi.waitFor(() => expect(finishRefresh).toBeTypeOf("function"));
  const latestRefresh = refreshNotificationProjects(["C:/queued"]);
  invoke.mockResolvedValueOnce({
    root: "C:/queued",
    commonDir: null,
    remote: "https://github.com/latest/queued.git",
  });
  finishRefresh({
    root: "C:/queued",
    commonDir: null,
    remote: "https://github.com/old/queued.git",
  });
  await Promise.all([firstRefresh, latestRefresh]);
  expect(knownNotificationProject("C:/queued")?.id).toBe(
    "repository:github.com/latest/queued",
  );
});

it("keeps native checkout identity when older Inbox items arrive later", async () => {
  invoke.mockResolvedValueOnce({
    root: "C:/inbox-race",
    commonDir: null,
    remote: "https://github.com/new/app.git",
  });
  await resolveNotificationProject("C:/inbox-race");
  rememberNotificationProjects([
    inboxNotificationProject({
      provider: "github",
      repo: "old/app",
      url: "https://github.com/old/app/pull/1",
      projectPath: "C:/inbox-race",
    }),
  ]);
  expect(knownNotificationProject("C:/inbox-race")?.id).toBe(
    "repository:github.com/new/app",
  );
  expect(
    loadNotificationProjects().find(
      (project) => project.id === "repository:github.com/old/app",
    )?.paths,
  ).toEqual([]);
});

it("rechecks Git changes that arrive during an older discovery", async () => {
  let finish!: (value: unknown) => void;
  invoke.mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  const initial = resolveNotificationProject("C:/racing");
  const refreshed = refreshNotificationProjects(["C:/racing"]);
  invoke.mockResolvedValueOnce({
    root: "C:/racing",
    commonDir: null,
    remote: "https://github.com/new/racing.git",
  });
  finish({
    root: "C:/racing",
    commonDir: null,
    remote: "https://github.com/old/racing.git",
  });
  await initial;
  await refreshed;
  expect(knownNotificationProject("C:/racing")?.id).toBe(
    "repository:github.com/new/racing",
  );
});

it("publishes discovered identities when persistent browser storage is unavailable", async () => {
  vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
    throw new Error("Quota exceeded");
  });
  invoke.mockResolvedValueOnce({
    root: "C:/memory",
    commonDir: null,
    remote: null,
  });
  await resolveNotificationProject("C:/memory");
  expect(
    knownNotificationProjectSelection(["C:/memory"])?.projects.map(
      (project) => project.id,
    ),
  ).toEqual(["local:c:/memory"]);
  expect((await resolveNotificationProject("C:/memory")).id).toBe(
    "local:c:/memory",
  );
  expect(invoke).toHaveBeenCalledTimes(1);
});

it("discards discovery started before the project catalog was cleared", async () => {
  invoke.mockResolvedValueOnce({
    root: "C:/profile",
    commonDir: null,
    remote: null,
  });
  await resolveNotificationProject("C:/profile");
  let finish!: (value: unknown) => void;
  invoke.mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  const previousRefresh = refreshNotificationProjects(["C:/profile"]).catch(
    () => {},
  );
  localStorage.clear();
  invoke.mockResolvedValueOnce({
    root: "C:/profile",
    commonDir: null,
    remote: "https://github.com/new/profile.git",
  });
  const current = resolveNotificationProject("C:/profile");
  try {
    expect(invoke).toHaveBeenCalledTimes(3);
    expect((await current).id).toBe("repository:github.com/new/profile");
  } finally {
    finish({
      root: "C:/profile",
      commonDir: null,
      remote: "https://github.com/old/profile.git",
    });
    await previousRefresh;
  }
  expect(knownNotificationProject("C:/profile")?.id).toBe(
    "repository:github.com/new/profile",
  );
});

it("retains known identity after refresh failure and retries after a short backoff", async () => {
  const now = vi.spyOn(Date, "now").mockReturnValue(1_000);
  invoke.mockResolvedValueOnce({
    root: "C:/retry",
    commonDir: null,
    remote: null,
  });
  const first = await resolveNotificationProject("C:/retry");
  now.mockReturnValue(61_001);
  invoke.mockRejectedValueOnce(new Error("Git unavailable"));
  await refreshNotificationProjects(["C:/retry"]);
  expect(await resolveNotificationProject("C:/retry")).toEqual(first);
  expect(invoke).toHaveBeenCalledTimes(2);
  now.mockReturnValue(66_002);
  invoke.mockResolvedValueOnce({
    root: "C:/retry",
    commonDir: null,
    remote: "https://github.com/acme/retry.git",
  });
  expect(await resolveNotificationProject("C:/retry")).toEqual(first);
  await vi.waitFor(() =>
    expect(knownNotificationProject("C:/retry")?.id).toBe(
      "repository:github.com/acme/retry",
    ),
  );
  expect(invoke).toHaveBeenCalledTimes(3);
});

it("refreshes a changed remote and detaches its path from the old Inbox identity", async () => {
  invoke.mockResolvedValueOnce({
    root: "C:/moved",
    commonDir: null,
    remote: "https://github.com/old/app.git",
  });
  await resolveNotificationProject("C:/moved");
  invoke.mockResolvedValueOnce({
    root: "C:/moved",
    commonDir: null,
    remote: "https://github.com/new/app.git",
  });
  await refreshNotificationProjects(["C:/moved", "c:\\moved"]);
  expect(knownNotificationProject("C:/moved")?.id).toBe(
    "repository:github.com/new/app",
  );
  expect(
    loadNotificationProjects().find(
      (project) => project.id === "repository:github.com/old/app",
    )?.paths,
  ).toEqual([]);
  expect(invoke).toHaveBeenCalledTimes(2);
});

it("refreshes stale identities without making known actions wait for Git", async () => {
  const now = vi.spyOn(Date, "now").mockReturnValue(1_000);
  const context = { root: "C:/stale", commonDir: null, remote: null };
  invoke.mockResolvedValueOnce(context);
  const known = await resolveNotificationProject("C:/stale");
  now.mockReturnValue(61_001);
  let finish!: (value: typeof context) => void;
  invoke.mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  expect(await resolveNotificationProject("C:/stale")).toEqual(known);
  expect(await resolveNotificationProject("C:/stale")).toEqual(known);
  expect(invoke).toHaveBeenCalledTimes(2);
  finish(context);
  await vi.waitFor(() =>
    expect(knownNotificationProjectSelection(["C:/stale"])).not.toBeNull(),
  );
});

it("exposes a synchronous bulk selection only when every checkout is known", async () => {
  rememberNotificationProjects([
    {
      id: "linear:project:inbox-only",
      name: "Inbox only",
      detail: "Linear",
      kind: "linear",
      paths: [],
    },
  ]);
  invoke.mockResolvedValue({ root: "C:/work", commonDir: null, remote: null });
  await resolveNotificationProject("C:/work");
  expect(
    knownNotificationProjectSelection(["C:/work", "D:/unknown"]),
  ).toBeNull();
  expect(
    knownNotificationProjectSelection([
      "C:/work",
      "c:\\work",
      "~",
    ])?.projects.map((project) => project.id),
  ).toEqual(["linear:project:inbox-only", "local:c:/work"]);
  expect(invoke).toHaveBeenCalledTimes(1);
});

it("reuses a resolved checkout identity across repeated actions", async () => {
  invoke.mockResolvedValue({
    root: "C:/cached",
    commonDir: "C:/cached/.git",
    remote: "https://github.com/acme/cached.git",
  });
  const first = await resolveNotificationProject("C:/cached");
  const second = await resolveNotificationProject("c:\\cached");
  expect(invoke).toHaveBeenCalledTimes(1);
  expect(second).toEqual(first);
});

it("keeps an unknown checkout unresolved when native discovery fails and allows retry", async () => {
  invoke.mockRejectedValueOnce(new Error("Native bridge unavailable"));
  await expect(resolveNotificationProject("C:/unknown")).rejects.toThrow(
    "Native bridge unavailable",
  );
  invoke.mockResolvedValueOnce({
    root: "C:/unknown",
    commonDir: null,
    remote: null,
  });
  expect((await resolveNotificationProject("C:/unknown")).id).toBe(
    "local:c:/unknown",
  );
});

it("collects Inbox projects and groups repository checkouts", async () => {
  rememberNotificationProjects([
    {
      id: "linear:project:roadmap",
      name: "Roadmap",
      detail: "Linear",
      kind: "linear",
      paths: [],
    },
  ]);
  invoke.mockResolvedValueOnce({
    root: "C:/work",
    commonDir: "C:/work/.git",
    remote: "git@github.com:company/app.git",
  });
  invoke.mockResolvedValueOnce({
    root: "D:/review",
    commonDir: "C:/work/.git",
    remote: "https://github.com/company/app.git",
  });
  invoke.mockResolvedValueOnce({
    root: "D:/personal",
    commonDir: "D:/personal/.git",
    remote: "https://github.com/me/personal.git",
  });
  const paths = ["C:/work", "D:/review", "D:/personal"];
  await Promise.all(paths.map(resolveNotificationProject));
  const selection = knownNotificationProjectSelection(paths)!;
  expect(selection.projects.map((project) => project.id).sort()).toEqual([
    "linear:project:roadmap",
    "repository:github.com/company/app",
    "repository:github.com/me/personal",
  ]);
  expect(
    selection.projects.find(
      (project) => project.id === "repository:github.com/company/app",
    )?.paths,
  ).toEqual(["C:/work", "D:/review"]);
});

it("shares repository preferences across checkouts, worktrees and remote Inbox items", async () => {
  invoke.mockResolvedValueOnce({
    root: "C:/main",
    commonDir: "C:/main/.git",
    remote: "git@github.com:Acme/App.git",
  });
  invoke.mockResolvedValueOnce({
    root: "D:/worktree",
    commonDir: "C:/main/.git",
    remote: "https://github.com/acme/app.git",
  });
  const first = await resolveNotificationProject("C:/main");
  const second = await resolveNotificationProject("D:/worktree");
  const item = inboxNotificationProject({
    provider: "github",
    repo: "acme/app",
    url: "https://github.com/acme/app/pull/5",
    projectPath: "C:/main",
  } as InboxItem);
  expect(first.id).toBe("repository:github.com/acme/app");
  expect(second.id).toBe(first.id);
  expect(item.id).toBe(first.id);
});

it("keeps Linear projects and unassigned teams separate from repositories and groups offline worktrees", async () => {
  const linear = (projectId: string, teamId = "team-a") =>
    inboxNotificationProject({
      provider: "linear",
      projectId,
      projectName: "Launch",
      teamId,
      teamName: "Product",
      repo: "PROD",
      url: "https://linear.app/acme/issue/PROD-1",
      projectPath: "",
    } as InboxItem);
  expect(linear("project-a").id).toBe("linear:project:project-a");
  expect(linear("").id).not.toBe(linear("", "team-b").id);
  invoke.mockResolvedValue({
    root: "D:/offline-worktree",
    commonDir: "C:/offline/.git",
    remote: null,
  });
  expect((await resolveNotificationProject("D:/offline-worktree")).id).toBe(
    "local:c:/offline/.git",
  );
});
