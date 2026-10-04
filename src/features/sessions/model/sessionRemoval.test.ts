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

function fixture(mode: "archive" | "delete") {
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
  let archiveChanged: boolean | undefined;
  const commit = vi.fn((removal: SessionWorkspaceRemoval) => {
    state = { ...state, ...removal };
  });
  mocks.invoke.mockImplementation(async (command, args) => {
    if (command === "session_set_archived") return true;
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
    commit,
    archiveChanged: () => archiveChanged,
    run: () => {
      const remover = createSessionRemover({
        mode,
        replacement: { cwd: "/tmp/project", harness: "cursor" },
        workspace: {
          snapshot: () => state,
          apply: (change) => {
            if (change.type === "orchestrationReleased") return;
            if (change.type === "removed") {
              archiveChanged = change.archivedChanged;
              commit(change.removal);
              return;
            }
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
    expect(f.stop).toHaveBeenCalledTimes(mode === "delete" ? 1 : 0);
    expect(f.commit).not.toHaveBeenCalled();
    expect(f.read().sessions[0]).toMatchObject({
      busy: false,
      ...(mode === "delete" ? { queueStatus: "paused" } : {}),
    });
    if (mode === "delete")
      expect(f.read().sessions[0].blocks[1].streaming).toBeFalsy();
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

it("archives an idle transcript without stopping or cancelling a worker", async () => {
  const f = fixture("archive");
  await f.run();
  const [command, args] = mocks.invoke.mock.calls[0];
  expect(command).toBe("session_upsert");
  expect(args.session.blocks.at(-1)).toMatchObject({
    text: "finished",
  });
  expect(args.session.blocks.at(-1).streaming).toBeFalsy();
  expect(f.stop).not.toHaveBeenCalled();
  expect(f.archiveChanged()).toBe(true);
  expect(mocks.invoke.mock.calls.map(([name]) => name)).toEqual([
    "session_upsert",
    "session_set_archived",
  ]);
});

it("rejects a busy archive before confirmation, cancellation, or storage", async () => {
  const f = fixture("archive");
  f.write({
    ...f.read(),
    sessions: f.read().sessions.map((session) =>
      session.id === f.closing.id ? { ...session, busy: true } : session,
    ),
  });
  expect(await f.run()).toBe(false);
  expect(f.confirmClose).not.toHaveBeenCalled();
  expect(f.stop).not.toHaveBeenCalled();
  expect(mocks.invoke).not.toHaveBeenCalled();
  expect(f.read().sessions[0].busy).toBe(true);
});
