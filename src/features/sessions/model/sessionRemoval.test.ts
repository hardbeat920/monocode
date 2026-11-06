import { afterEach, describe, expect, it, vi } from "vitest";
import {
  leafIds,
  newFileTab,
  newTab,
  openEditorTab,
  newTerminalFile,
  openTerminalTab,
} from "../../workspace/model/layout";
import { newSession, type Session } from "./session";
import type { SessionWorkspaceRemoval } from "./sessionWorkspaceLifecycle";
import { createSessionRemover } from "./sessionRemoval";

const mocks = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));

function deferred() {
  let resolve!: () => void;
  const promise = new Promise<void>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

function fixture(
  mode: "archive" | "delete",
  preserveOpenFiles = false,
  archiveIdleOnly = false,
) {
  const busy = mode === "delete";
  const closing: Session = {
    ...newSession("cursor", "/tmp/project"),
    busy,
    blocks: [
      { id: "user", role: "user", text: "hello" },
      {
        id: "answer",
        role: "assistant",
        text: busy ? "partial" : "finished",
        ...(busy ? { streaming: true } : {}),
      },
    ],
    ...(busy
      ? {
          queuedMessages: [{ id: "queued", text: "next", attachments: [] }],
          queueStatus: "active" as const,
        }
      : {}),
  };
  const other = newSession("cursor", "/tmp/project");
  let state = {
    tabs: [newTab(closing.id), newTab(other.id)],
    sessions: [closing, other],
    activeTabId: "",
    dirtyFiles: new Set<string>(),
  };
  state.activeTabId = state.tabs[0].id;
  const confirmClose = vi.fn(async () => true);
  const stop = vi.fn(async () => {});
  let stopped: Session | undefined;
  let archived = false;
  let archivedChanged: boolean | undefined;
  const commit = vi.fn((removal: SessionWorkspaceRemoval) => {
    state = { ...state, ...removal };
  });
  mocks.invoke.mockImplementation(async (command, args) => {
    if (command === "session_set_archived") {
      const changed = archived !== args.archived;
      archived = args.archived;
      return changed;
    }
    if (command === "session_upsert") {
      return { ...args.session, createdAt: 1, updatedAt: 1 };
    }
  });
  return {
    closing,
    other,
    read: () => state,
    write: (next: typeof state) => {
      state = next;
    },
    confirmClose,
    stop,
    stopped: () => stopped,
    commit,
    archiveChanged: () => archivedChanged,
    run: () => {
      const remover = createSessionRemover({
        mode,
        archiveIdleOnly,
        preserveOpenFiles,
        replacement: { cwd: "/tmp/project", harness: "cursor" },
        workspace: {
          snapshot: () => state,
          apply: (change) => {
            if (change.type === "orchestrationReleased") return;
            if (change.type === "removed") {
              archivedChanged = change.archivedChanged;
              commit(change.removal);
              return;
            }
            stopped = change.session;
            state = {
              ...state,
              sessions: state.sessions.map((session) =>
                session.id === change.session.id ? change.session : session,
              ),
            };
          },
        },
        confirm: confirmClose,
        stop,
      });
      return remover.remove(closing.id);
    },
  };
}

afterEach(() => mocks.invoke.mockReset());

describe.each(["archive", "delete"] as const)("%s lifecycle", (mode) => {
  it("keeps dirty files and terminals open when removing through the CLI", async () => {
    const f = fixture(mode, true);
    const file = newFileTab("/tmp/project/unsaved.ts", "/tmp/project");
    const terminal = newTerminalFile("/tmp/project");
    const tab = openTerminalTab(
      openEditorTab(f.read().tabs[0], file, { pin: true }),
      terminal,
    );
    f.write({
      ...f.read(),
      tabs: [tab, f.read().tabs[1]],
      dirtyFiles: new Set([file.id]),
    });
    expect(await f.run()).toBe(true);
    expect(f.confirmClose).toHaveBeenCalledExactlyOnceWith([], mode);
    const kept = f.read().tabs.find((entry) => entry.id === tab.id)!;
    expect(kept.editorPanes[0].files).toContain(file);
    expect(kept.terminalPanes[0].files).toContain(terminal);
    expect(f.read().dirtyFiles.has(file.id)).toBe(true);
    expect(leafIds(kept.layout)).not.toContain(f.closing.id);
    expect(f.read().sessions.map((session) => session.id)).not.toContain(
      f.closing.id,
    );
    expect(f.read().sessions.find((session) => session.id === f.other.id)).toBe(
      f.other,
    );
    expect(f.commit.mock.calls[0][0].closedTabs).toEqual([]);
  });

  it("creates the replacement session behind the removal interface", async () => {
    const f = fixture(mode);
    const onlyTab = f.read().tabs[0];
    f.write({
      ...f.read(),
      tabs: [onlyTab],
      sessions: [f.closing],
      activeTabId: onlyTab.id,
    });

    expect(await f.run()).toBe(true);
    expect(f.read().sessions).toHaveLength(1);
    expect(f.read().sessions[0]).toMatchObject({
      cwd: "/tmp/project",
      harness: "cursor",
      blocks: [],
    });
    expect(f.read().sessions[0].id).not.toBe(f.closing.id);
    expect(leafIds(f.read().tabs[0].layout)).toEqual([f.read().sessions[0].id]);
  });

  it("preserves another agent's completion, new tabs, and focus across both waits", async () => {
    const f = fixture(mode);
    const confirm = deferred();
    const saving = deferred();
    f.confirmClose.mockImplementation(async () => {
      await confirm.promise;
      return true;
    });
    mocks.invoke.mockImplementation(async (command, args) => {
      await saving.promise;
      return command === "session_upsert"
        ? { ...args.session, createdAt: 1, updatedAt: 1 }
        : undefined;
    });
    const pending = f.run();
    const updated = {
      ...f.other,
      busy: false,
      blocks: [{ id: "done", role: "assistant" as const, text: "finished" }],
    };
    f.write({ ...f.read(), sessions: [f.closing, updated] });
    confirm.resolve();
    await vi.waitFor(() => expect(mocks.invoke).toHaveBeenCalled());
    const added = newSession("cursor", "/tmp/project");
    const addedTab = newTab(added.id);
    f.write({
      ...f.read(),
      tabs: [...f.read().tabs, addedTab],
      sessions: [...f.read().sessions, added],
      activeTabId: addedTab.id,
    });
    saving.resolve();
    await pending;
    expect(f.read().sessions).toEqual([updated, added]);
    expect(f.read().activeTabId).toBe(addedTab.id);
    expect(f.read().tabs.map((tab) => tab.id)).toContain(addedTab.id);
  });

  it("retains a stopped session and paused queue when storage fails", async () => {
    const f = fixture(mode);
    mocks.invoke.mockRejectedValue(new Error("disk unavailable"));
    await expect(f.run()).rejects.toThrow("disk unavailable");
    expect(f.stop).toHaveBeenCalledOnce();
    expect(f.commit).not.toHaveBeenCalled();
    expect(f.read().sessions[0].busy).toBe(false);
    if (mode === "delete") {
      expect(f.read().sessions[0].queueStatus).toBe("paused");
      expect(f.read().sessions[0].blocks[1].streaming).toBeFalsy();
    }
    expect(f.read().tabs).toHaveLength(2);
  });

  it("does not stop or write anything if confirmation is declined", async () => {
    const f = fixture(mode);
    f.confirmClose.mockResolvedValue(false);
    expect(await f.run()).toBe(false);
    expect(f.stop).not.toHaveBeenCalled();
    expect(mocks.invoke).not.toHaveBeenCalled();
    expect(f.read().sessions[0].busy).toBe(mode === "delete");
  });

  it("keeps files that become dirty while storage is pending", async () => {
    const f = fixture(mode);
    const file = newFileTab("/tmp/project/new.ts", "/tmp/project");
    f.write({
      ...f.read(),
      tabs: [
        openEditorTab(f.read().tabs[0], file, { pin: true }),
        f.read().tabs[1],
      ],
    });
    mocks.invoke.mockImplementation(async (command, args) => {
      f.write({ ...f.read(), dirtyFiles: new Set([file.id]) });
      return command === "session_upsert"
        ? { ...args.session, createdAt: 1, updatedAt: 1 }
        : undefined;
    });
    await f.run();
    expect(f.read().sessions.map((s) => s.id)).not.toContain(f.closing.id);
    expect(f.read().tabs[0].editorPanes[0].files).toContain(file);
    expect(leafIds(f.read().tabs[0].layout)).not.toContain(f.closing.id);
    expect(f.commit.mock.calls[0][0].closedTabs).toEqual([]);
  });

  it("keeps a terminal opened while confirmation is pending", async () => {
    const f = fixture(mode);
    const terminal = newTerminalFile("/tmp/project");
    f.confirmClose.mockImplementation(async () => {
      f.write({
        ...f.read(),
        tabs: [openTerminalTab(f.read().tabs[0], terminal), f.read().tabs[1]],
      });
      return true;
    });
    await f.run();
    expect(f.read().tabs[0].terminalPanes[0].files).toContain(terminal);
    expect(f.commit.mock.calls[0][0].closedTabs).toEqual([]);
  });
});

it("archives an idle transcript and reports whether the archive state changed", async () => {
  const f = fixture("archive");
  await f.run();
  const [command, args] = mocks.invoke.mock.calls[0];
  expect(command).toBe("session_upsert");
  expect(args.session.blocks.at(-1)).toMatchObject({ text: "finished" });
  expect(args.session.blocks.at(-1).streaming).toBeFalsy();
  expect(f.stop).toHaveBeenCalledExactlyOnceWith(f.closing.id);
  expect(f.archiveChanged()).toBe(true);
  expect(mocks.invoke.mock.calls.map(([name]) => name)).toEqual([
    "session_upsert",
    "session_set_archived",
  ]);
  await f.run();
  expect(f.archiveChanged()).toBe(false);
});

it("archives an idle API target without calling the stop path", async () => {
  const f = fixture("archive", false, true);
  expect(await f.run()).toBe(true);
  expect(f.stop).not.toHaveBeenCalled();
  expect(f.archiveChanged()).toBe(true);
});

it("keeps a target active if it becomes busy during a later archive write", async () => {
  const f = fixture("archive", false, true);
  const queuedMessages = [{ id: "queued", text: "next", attachments: [] }];
  f.write({
    ...f.read(),
    sessions: f
      .read()
      .sessions.map((session) =>
        session.id === f.closing.id
          ? { ...session, queuedMessages, queueStatus: "active" as const }
          : session,
      ),
  });
  const saving = deferred();
  mocks.invoke.mockImplementation(async (command, args) => {
    if (command === "session_upsert") {
      await saving.promise;
      return { ...args.session, createdAt: 1, updatedAt: 1 };
    }
    return false;
  });
  const pending = f.run();
  await vi.waitFor(() => expect(mocks.invoke).toHaveBeenCalledOnce());
  f.write({
    ...f.read(),
    sessions: f.read().sessions.map((session) =>
      session.id === f.closing.id
        ? {
            ...session,
            busy: true,
            blocks: [
              ...session.blocks,
              {
                id: "later-turn",
                role: "user" as const,
                text: "new activity",
              },
            ],
          }
        : session,
    ),
  });
  saving.resolve();
  expect(await pending).toBe(false);
  expect(f.stop).not.toHaveBeenCalled();
  expect(mocks.invoke.mock.calls.map(([name]) => name)).toEqual([
    "session_upsert",
  ]);
  expect(f.read().sessions[0].busy).toBe(true);
  expect(f.read().sessions[0].queueStatus).toBe("active");
  expect(f.read().sessions[0].queuedMessages).toEqual(queuedMessages);
  expect(f.read().tabs).toHaveLength(2);
});

it("keeps queued work active when an API archive write fails", async () => {
  const f = fixture("archive", false, true);
  const queuedMessages = [{ id: "queued", text: "next", attachments: [] }];
  f.write({
    ...f.read(),
    sessions: f
      .read()
      .sessions.map((session) =>
        session.id === f.closing.id
          ? { ...session, queuedMessages, queueStatus: "active" as const }
          : session,
      ),
  });
  mocks.invoke.mockRejectedValue(new Error("disk unavailable"));
  await expect(f.run()).rejects.toThrow("disk unavailable");
  expect(f.stop).not.toHaveBeenCalled();
  expect(f.read().sessions[0]).toMatchObject({
    queueStatus: "active",
    queuedMessages,
  });
  expect(f.commit).not.toHaveBeenCalled();
});

it("undoes an archive flag if the target becomes busy while it is being stored", async () => {
  const f = fixture("archive", false, true);
  const archiving = deferred();
  mocks.invoke.mockImplementation(async (command, args) => {
    if (command === "session_upsert")
      return { ...args.session, createdAt: 1, updatedAt: 1 };
    if (command === "session_set_archived") {
      if (args.archived) await archiving.promise;
      return true;
    }
  });
  const pending = f.run();
  await vi.waitFor(() =>
    expect(mocks.invoke).toHaveBeenCalledWith("session_set_archived", {
      sessionId: f.closing.id,
      archived: true,
    }),
  );
  f.write({
    ...f.read(),
    sessions: f
      .read()
      .sessions.map((session) =>
        session.id === f.closing.id ? { ...session, busy: true } : session,
      ),
  });
  archiving.resolve();
  expect(await pending).toBe(false);
  expect(
    mocks.invoke.mock.calls.map(([name, args]) => [name, args.archived]),
  ).toEqual([
    ["session_upsert", undefined],
    ["session_set_archived", true],
    ["session_set_archived", false],
  ]);
  expect(f.commit).not.toHaveBeenCalled();
  expect(f.read().sessions[0].busy).toBe(true);
});

it("surfaces a failed archive rollback after saving the transcript", async () => {
  const f = fixture("archive", false, true);
  const archiving = deferred();
  mocks.invoke.mockImplementation(async (command, args) => {
    if (command === "session_upsert")
      return { ...args.session, createdAt: 1, updatedAt: 1 };
    if (command === "session_set_archived") {
      if (args.archived) {
        await archiving.promise;
        return true;
      }
      throw new Error("rollback disk unavailable");
    }
  });
  const pending = f.run();
  await vi.waitFor(() =>
    expect(mocks.invoke).toHaveBeenCalledWith("session_set_archived", {
      sessionId: f.closing.id,
      archived: true,
    }),
  );
  f.write({
    ...f.read(),
    sessions: f
      .read()
      .sessions.map((session) =>
        session.id === f.closing.id ? { ...session, busy: true } : session,
      ),
  });
  archiving.resolve();
  await expect(pending).rejects.toThrow("rollback disk unavailable");
  expect(mocks.invoke).toHaveBeenCalledWith("session_upsert", {
    session: expect.objectContaining({ id: f.closing.id }),
  });
  expect(f.commit).not.toHaveBeenCalled();
  expect(f.read().sessions[0].busy).toBe(true);
});

it("keeps ordinary UI archive able to stop a busy session", async () => {
  const f = fixture("archive");
  f.write({
    ...f.read(),
    sessions: f.read().sessions.map((session) =>
      session.id === f.closing.id
        ? {
            ...session,
            busy: true,
            blocks: [
              { id: "user", role: "user" as const, text: "hello" },
              {
                id: "answer",
                role: "assistant" as const,
                text: "partial",
                streaming: true,
              },
            ],
            queuedMessages: [{ id: "queued", text: "next", attachments: [] }],
            queueStatus: "active" as const,
          }
        : session,
    ),
  });
  await expect(f.run()).resolves.toBe(true);
  expect(f.stop).toHaveBeenCalledExactlyOnceWith(f.closing.id);
  expect(f.stopped()?.queueStatus).toBe("paused");
  expect(f.commit).toHaveBeenCalledOnce();
});

it("does not stop an API archive target that is already busy or turns busy during confirmation", async () => {
  const alreadyBusy = fixture("archive", false, true);
  alreadyBusy.write({
    ...alreadyBusy.read(),
    sessions: alreadyBusy
      .read()
      .sessions.map((session) =>
        session.id === alreadyBusy.closing.id
          ? { ...session, busy: true }
          : session,
      ),
  });
  expect(await alreadyBusy.run()).toBe(false);
  expect(alreadyBusy.confirmClose).not.toHaveBeenCalled();
  expect(alreadyBusy.stop).not.toHaveBeenCalled();

  const racesBusy = fixture("archive", false, true);
  racesBusy.confirmClose.mockImplementation(async () => {
    racesBusy.write({
      ...racesBusy.read(),
      sessions: racesBusy
        .read()
        .sessions.map((session) =>
          session.id === racesBusy.closing.id
            ? { ...session, busy: true }
            : session,
        ),
    });
    return true;
  });
  expect(await racesBusy.run()).toBe(false);
  expect(racesBusy.stop).not.toHaveBeenCalled();
  expect(mocks.invoke).not.toHaveBeenCalled();
  expect(racesBusy.read().sessions[0].busy).toBe(true);
  expect(racesBusy.read().tabs).toHaveLength(2);
});
