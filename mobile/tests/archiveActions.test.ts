import { beforeEach, describe, expect, it, vi } from "vitest";

const alert = vi.hoisted(() => vi.fn());
vi.mock("react-native", () => ({ Alert: { alert } }));

import {
  archiveMany,
  archiveWithUndo,
  confirmArchive,
  confirmArchiveMany,
} from "@/features/sessions/archiveActions";

const session = { id: "s1", title: "Fix the build" };

function toasts() {
  const shown: { message: string; undo?: () => void }[] = [];
  return {
    shown,
    sink: {
      show: vi.fn((message: string, undo?: () => void) => void shown.push({ message, undo })),
      dismiss: vi.fn(),
    },
    last: () => shown[shown.length - 1],
  };
}
const deferred = <T = void>() => {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((done, fail) => {
    resolve = done;
    reject = fail;
  });
  return { promise, resolve, reject };
};
const flush = () => new Promise((done) => setTimeout(done, 0));

beforeEach(() => alert.mockClear());

describe("archiving a session from the list", () => {
  it("says so at once, before the desktop answers, with Undo when it can unarchive", async () => {
    const t = toasts();
    const answer = deferred();
    const archive = vi.fn(() => answer.promise);
    const done = archiveWithUndo(session, { archive, unarchive: vi.fn(async () => {}) }, t.sink);
    expect(archive).toHaveBeenCalledWith("s1");
    expect(t.last().message).toBe("Archived “Fix the build”");
    expect(t.last().undo).toBeTypeOf("function");
    answer.resolve();
    await done;
    expect(t.shown).toHaveLength(1);
  });

  it("offers no Undo when the desktop can't unarchive: it never fakes one", async () => {
    const t = toasts();
    await archiveWithUndo(session, { archive: vi.fn(async () => {}) }, t.sink);
    expect(t.last()).toEqual({ message: "Archived “Fix the build”", undo: undefined });
  });

  it("says it is back in the list when the desktop refuses, and drops the Undo", async () => {
    const t = toasts();
    await archiveWithUndo(
      session,
      { archive: vi.fn(async () => Promise.reject(new Error("Desktop is busy"))), unarchive: vi.fn() },
      t.sink,
    );
    expect(t.last().message).toContain("Couldn’t archive “Fix the build”. It’s back in the list.");
    expect(t.last().message).toContain("Desktop is busy");
    expect(t.last().undo).toBeUndefined();
  });

  it("Undo unarchives the same session and closes the toast", async () => {
    const t = toasts();
    const unarchive = vi.fn(async () => {});
    await archiveWithUndo(session, { archive: vi.fn(async () => {}), unarchive }, t.sink);
    t.shown[0].undo?.();
    await flush();
    expect(t.sink.dismiss).toHaveBeenCalled();
    expect(unarchive).toHaveBeenCalledWith("s1");
    expect(t.shown).toHaveLength(1);
  });

  it("an Undo pressed before the archive landed waits for it, so they reach the desktop in order", async () => {
    const t = toasts();
    const answer = deferred();
    const calls: string[] = [];
    const done = archiveWithUndo(
      session,
      {
        archive: vi.fn(async () => {
          await answer.promise;
          calls.push("archive");
        }),
        unarchive: vi.fn(async () => void calls.push("unarchive")),
      },
      t.sink,
    );
    t.shown[0].undo?.();
    await flush();
    expect(calls).toEqual([]);
    answer.resolve();
    await done;
    await flush();
    expect(calls).toEqual(["archive", "unarchive"]);
  });

  it("an Undo the desktop fails says the session is still archived", async () => {
    const t = toasts();
    await archiveWithUndo(
      session,
      { archive: vi.fn(async () => {}), unarchive: vi.fn(async () => Promise.reject(new Error("Unknown method"))) },
      t.sink,
    );
    t.shown[0].undo?.();
    await flush();
    expect(t.last().message).toContain("Couldn’t undo. “Fix the build” is still archived.");
  });
});

describe("archiving a Mono's finished sessions", () => {
  it("archives each and counts them", async () => {
    const t = toasts();
    const archive = vi.fn(async (_id: string) => {});
    await archiveMany([{ id: "a" }, { id: "b" }, { id: "c" }], { archive }, t.sink);
    expect(archive.mock.calls.map(([id]) => id)).toEqual(["a", "b", "c"]);
    expect(t.last()).toEqual({ message: "Archived 3 sessions", undo: undefined });
    await archiveMany([{ id: "a" }], { archive }, t.sink);
    expect(t.last().message).toBe("Archived 1 session");
  });

  it("reports the ones the desktop refused, which the workspace has already put back", async () => {
    const t = toasts();
    const archive = vi.fn(async (id: string) => {
      if (id === "b") throw new Error("nope");
    });
    await archiveMany([{ id: "a" }, { id: "b" }], { archive }, t.sink);
    expect(t.last().message).toBe("Archived 1 of 2. 1 couldn’t be, and are back in the list.");
    expect(t.last().undo).toBeUndefined();
  });
});

describe("asking before an archive", () => {
  it("uses the session menu's words and archives only on Archive", () => {
    const onConfirm = vi.fn();
    confirmArchive({ title: "Fix the build" }, onConfirm);
    const [title, message, buttons] = alert.mock.calls[0];
    expect(title).toBe("Archive “Fix the build”?");
    expect(message).toBe("It moves to the archive on your desktop.");
    expect(buttons.map((button: { text: string }) => button.text)).toEqual(["Cancel", "Archive"]);
    expect(onConfirm).not.toHaveBeenCalled();
    buttons[1].onPress();
    expect(onConfirm).toHaveBeenCalledOnce();
  });

  it("names the count and the Mono for a batch", () => {
    confirmArchiveMany(2, "TaskGiver", vi.fn());
    expect(alert.mock.calls[0][0]).toBe("Archive 2 finished sessions from TaskGiver?");
    confirmArchiveMany(1, "TaskGiver", vi.fn());
    expect(alert.mock.calls[1][0]).toBe("Archive 1 finished session from TaskGiver?");
  });
});
