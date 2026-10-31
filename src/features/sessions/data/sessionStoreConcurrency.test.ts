import { afterEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({ invoke: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));

type Deferred<T> = {
  promise: Promise<T>;
  resolve: (value: T) => void;
};

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((res) => {
    resolve = res;
  });
  return { promise, resolve };
}

async function loadStore() {
  vi.resetModules();
  return import("./sessionStore");
}

function session(id: string) {
  return {
    id,
    cwd: "/tmp/project",
    harness: "cursor" as const,
    model: "",
    modelSettings: {},
    runtimeMode: "supervised" as const,
    title: "",
    blocks: [{ id: "user", role: "user" as const, text: "hello" }],
    busy: true,
  };
}

afterEach(() => {
  mocks.invoke.mockReset();
});

describe("session persistence concurrency", () => {
  it("drains worker writes before deleting a lead and strips ownership from later snapshots", async () => {
    const firstWrite = deferred<unknown>();
    const commands: string[] = [];
    mocks.invoke.mockImplementation((command: string) => {
      commands.push(command);
      return command === "session_upsert_tail" && commands.length === 1
        ? firstWrite.promise
        : Promise.resolve();
    });
    const { deleteSession, upsertSession } = await loadStore();
    const worker = { ...session("worker"), orchestrationLeadId: "lead" };
    const writing = upsertSession(worker);
    await vi.waitFor(() => expect(commands).toEqual(["session_upsert_tail"]));
    const queued = upsertSession(worker);
    const deleting = deleteSession("lead");
    expect(commands).toEqual(["session_upsert_tail"]);
    firstWrite.resolve(undefined);
    await Promise.all([writing, queued, deleting]);
    expect(commands).toEqual([
      "session_upsert_tail",
      "session_upsert_tail",
      "session_delete",
    ]);
    await upsertSession(worker);
    for (const [, args] of mocks.invoke.mock.calls
      .slice(1)
      .filter(([command]) => command === "session_upsert_tail")) {
      expect(args.session.blocks[0]?.orchestrationLeadId).toBeUndefined();
    }
  });

  it("serializes deletion after an active write and drops a queued late upsert", async () => {
    const firstWrite = deferred<unknown>();
    const commands: string[] = [];
    mocks.invoke.mockImplementation((command: string) => {
      commands.push(command);
      if (command === "session_upsert_tail") return firstWrite.promise;
      return Promise.resolve();
    });
    const { deleteSession, upsertSession } = await loadStore();

    const writing = upsertSession(session("s1"));
    await vi.waitFor(() => expect(commands).toEqual(["session_upsert_tail"]));
    const lateWrite = upsertSession({ ...session("s1"), title: "late" });
    const deleting = deleteSession("s1");

    expect(commands).toEqual(["session_upsert_tail"]);
    firstWrite.resolve({
      id: "s1",
      cwd: "/tmp/project",
      harness: "cursor",
      model: "",
      runtimeMode: "supervised",
      title: "",
      createdAt: 1,
      updatedAt: 1,
    });
    await Promise.all([writing, lateWrite, deleting]);

    expect(commands).toEqual(["session_upsert_tail", "session_delete"]);
  });

  it("discards a draft-only record before reusing its open session id", async () => {
    const firstWrite = deferred<unknown>();
    const commands: string[] = [];
    mocks.invoke.mockImplementation((command: string) => {
      commands.push(command);
      if (commands.length === 1) return firstWrite.promise;
      return Promise.resolve({
        id: "s1",
        cwd: "/tmp/project",
        harness: "cursor",
        model: "",
        runtimeMode: "supervised",
        title: "",
        createdAt: 1,
        updatedAt: 1,
      });
    });
    const { discardDraftSessionRecord, upsertSession } = await loadStore();

    const draftWrite = upsertSession(session("s1"));
    await vi.waitFor(() => expect(commands).toEqual(["session_upsert_tail"]));
    const discarding = discardDraftSessionRecord("s1");
    const laterTurn = upsertSession({ ...session("s1"), title: "later" });

    firstWrite.resolve(undefined);
    await Promise.all([draftWrite, discarding, laterTurn]);
    expect(commands).toEqual([
      "session_upsert_tail",
      "session_delete",
      "session_upsert_tail",
    ]);
  });

  it("drains queued saves before detaching a removed worktree", async () => {
    const firstWrite = deferred<unknown>();
    mocks.invoke
      .mockReturnValueOnce(firstWrite.promise)
      .mockResolvedValue(undefined);
    const { flushSessionWrites, upsertSession } = await loadStore();
    const active = upsertSession(session("s1"));
    await vi.waitFor(() => expect(mocks.invoke).toHaveBeenCalledOnce());
    const queued = upsertSession({ ...session("s1"), title: "latest" });
    let flushed = false;
    const flushing = flushSessionWrites().then(() => {
      flushed = true;
    });
    await Promise.resolve();
    expect(flushed).toBe(false);
    firstWrite.resolve(undefined);
    await Promise.all([active, queued, flushing]);
    expect(mocks.invoke).toHaveBeenCalledTimes(2);
    expect(flushed).toBe(true);
  });

  it("archives only after the final active-turn snapshot is durable", async () => {
    const firstWrite = deferred<unknown>();
    const commands: string[] = [];
    mocks.invoke.mockImplementation((command: string) => {
      commands.push(command);
      if (command === "session_upsert_tail" && commands.length === 1) {
        return firstWrite.promise;
      }
      return Promise.resolve({
        id: "s1",
        cwd: "/tmp/project",
        harness: "cursor",
        model: "",
        runtimeMode: "supervised",
        title: "",
        createdAt: 1,
        updatedAt: 1,
      });
    });
    const { setSessionArchived, upsertSession } = await loadStore();

    const writing = upsertSession(session("s1"));
    await vi.waitFor(() => expect(commands).toEqual(["session_upsert_tail"]));
    const finalSnapshot = upsertSession({
      ...session("s1"),
      blocks: [
        { id: "user", role: "user", text: "hello" },
        { id: "assistant", role: "assistant", text: "final buffered output" },
      ],
    });
    const archiving = setSessionArchived("s1", true);

    expect(commands).toEqual(["session_upsert_tail"]);
    firstWrite.resolve({
      id: "s1",
      cwd: "/tmp/project",
      harness: "cursor",
      model: "",
      runtimeMode: "supervised",
      title: "",
      createdAt: 1,
      updatedAt: 1,
    });
    await Promise.all([writing, finalSnapshot, archiving]);

    expect(commands).toEqual([
      "session_upsert_tail",
      "session_upsert_tail",
      "session_set_archived",
    ]);
    expect(mocks.invoke.mock.calls[1]?.[1]).toMatchObject({
      session: {
        blocks: [{ text: "hello" }, { text: "final buffered output" }],
      },
    });
  });
});

describe("regular session delta persistence", () => {
  function record(id: string) {
    return {
      id,
      cwd: "/tmp/project",
      harness: "cursor",
      model: "",
      modelSettings: {},
      runtimeMode: "supervised",
      title: "",
      createdAt: 1,
      updatedAt: 2,
      blocks: [
        { id: "u", role: "user", text: "Recent question" },
        { id: "a", role: "assistant", text: "Recent answer" },
      ],
    };
  }

  function summary(session: { id: string; cwd: string; title: string }) {
    return {
      id: session.id,
      cwd: session.cwd,
      harness: "cursor",
      model: "",
      runtimeMode: "supervised",
      title: session.title,
      createdAt: 1,
      updatedAt: 2,
    };
  }

  async function load(id = "s1") {
    const { readSession, upsertSession } = await loadStore();
    mocks.invoke.mockResolvedValueOnce(record(id));
    const session = (await readSession(id))!;
    return { session, upsertSession };
  }

  it("writes only the appended suffix after the anchored tail", async () => {
    const { session, upsertSession } = await load();
    session.blocks = [
      ...session.blocks,
      { id: "next", role: "user", text: "Next" },
    ];
    mocks.invoke.mockResolvedValueOnce(summary(session));
    await upsertSession(session);
    expect(mocks.invoke).toHaveBeenLastCalledWith(
      "session_upsert_tail",
      expect.objectContaining({
        afterBlockId: "a",
        fromBlockId: null,
        appendOnly: false,
        blocksChanged: true,
        session: expect.objectContaining({
          blocks: [expect.objectContaining({ id: "next" })],
        }),
      }),
    );
  });

  it("anchors a rewritten head block on the block it replaces", async () => {
    const { session, upsertSession } = await load();
    session.blocks = [
      { ...session.blocks[0], text: "Edited" },
      ...session.blocks.slice(1),
    ];
    mocks.invoke.mockResolvedValueOnce(summary(session));
    await upsertSession(session);
    expect(mocks.invoke).toHaveBeenLastCalledWith(
      "session_upsert_tail",
      expect.objectContaining({
        afterBlockId: null,
        fromBlockId: "u",
        appendOnly: false,
        blocksChanged: true,
        session: expect.objectContaining({
          blocks: [
            expect.objectContaining({ id: "u", text: "Edited" }),
            expect.objectContaining({ id: "a" }),
          ],
        }),
      }),
    );
  });

  it("serializes overlapping saves against the last successful suffix", async () => {
    const { session, upsertSession } = await load();
    const first = {
      ...session,
      blocks: [
        ...session.blocks,
        { id: "one", role: "user" as const, text: "One" },
      ],
    };
    const second = {
      ...first,
      blocks: [
        ...first.blocks,
        { id: "two", role: "assistant" as const, text: "Two" },
      ],
    };
    mocks.invoke.mockResolvedValue(summary(second));
    await Promise.all([upsertSession(first), upsertSession(second)]);
    const tails = mocks.invoke.mock.calls
      .filter(([command]) => command === "session_upsert_tail")
      .map(([, args]) => args as { afterBlockId: string });
    expect(tails.map((args) => args.afterBlockId)).toEqual(["a", "one"]);
    expect(mocks.invoke).toHaveBeenLastCalledWith(
      "session_upsert_tail",
      expect.objectContaining({
        session: expect.objectContaining({
          blocks: [expect.objectContaining({ id: "two" })],
        }),
      }),
    );
  });

  it("sends a header-only update when the transcript is unchanged", async () => {
    const { session, upsertSession } = await load();
    session.title = "Renamed";
    mocks.invoke.mockResolvedValueOnce(summary(session));
    await upsertSession(session);
    expect(mocks.invoke).toHaveBeenLastCalledWith(
      "session_upsert_tail",
      expect.objectContaining({
        blocksChanged: false,
        session: expect.objectContaining({ title: "Renamed", blocks: [] }),
      }),
    );
  });

  it("retries a failed tail write as the full upsert instead of losing history", async () => {
    const { session, upsertSession } = await load();
    session.blocks = [
      ...session.blocks,
      { id: "next", role: "user" as const, text: "Next" },
    ];
    mocks.invoke
      .mockRejectedValueOnce(new Error("Transcript anchor is gone"))
      .mockResolvedValueOnce(summary(session));
    await upsertSession(session);
    expect(mocks.invoke).toHaveBeenCalledTimes(3);
    expect(mocks.invoke).toHaveBeenLastCalledWith(
      "session_upsert",
      expect.objectContaining({
        session: expect.objectContaining({
          blocks: [
            expect.objectContaining({ id: "u" }),
            expect.objectContaining({ id: "a" }),
            expect.objectContaining({ id: "next" }),
          ],
        }),
      }),
    );
    // After the full write lands, the next save resumes tail-only deltas.
    session.blocks = [
      ...session.blocks,
      { id: "later", role: "assistant" as const, text: "Later" },
    ];
    mocks.invoke.mockResolvedValueOnce(summary(session));
    await upsertSession(session);
    expect(mocks.invoke).toHaveBeenLastCalledWith(
      "session_upsert_tail",
      expect.objectContaining({
        afterBlockId: "next",
        blocksChanged: true,
        session: expect.objectContaining({
          blocks: [expect.objectContaining({ id: "later" })],
        }),
      }),
    );
  });

  it("resends from the first user block when its orchestration lead stamp changes", async () => {
    const { session, upsertSession } = await load();
    const stamped = { ...session, orchestrationLeadId: "lead" };
    mocks.invoke.mockResolvedValueOnce(summary(stamped));
    await upsertSession(stamped);
    expect(mocks.invoke).toHaveBeenLastCalledWith(
      "session_upsert_tail",
      expect.objectContaining({
        afterBlockId: null,
        fromBlockId: "u",
        session: expect.objectContaining({
          blocks: [
            expect.objectContaining({ id: "u", orchestrationLeadId: "lead" }),
            expect.objectContaining({ id: "a" }),
          ],
        }),
      }),
    );
    // A steady stamp needs no resend; an unchanged transcript stays header-only.
    const grown = {
      ...stamped,
      blocks: [
        ...stamped.blocks,
        { id: "next", role: "assistant" as const, text: "Next" },
      ],
    };
    mocks.invoke.mockResolvedValueOnce(summary(grown));
    await upsertSession(grown);
    expect(mocks.invoke).toHaveBeenLastCalledWith(
      "session_upsert_tail",
      expect.objectContaining({
        afterBlockId: "a",
        blocksChanged: true,
        session: expect.objectContaining({
          blocks: [expect.objectContaining({ id: "next" })],
        }),
      }),
    );
  });
});
