// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from "vitest";
import { appendUser } from "../../../integrations/harness/core/apply";
import {
  deleteSession,
  getSession,
  upsertSession,
} from "../../sessions/data/sessionStore";
import { newSession, type Session } from "../../sessions/model/session";
import {
  createEditedResendAttempt,
  submitPreservedEditedResend,
} from "../../sessions/model/editLastTurn";
import {
  persistAcceptedAssignment,
  repairAssignmentReceipts,
  type AssignmentReceiptHost,
} from "./assignments";

const transport = vi.hoisted(() => ({
  records: new Map<string, unknown>(),
  writes: [] as { id: string; blocks: unknown[] }[],
  failParent: false,
  failReads: new Set<string>(),
  waitParent: undefined as Promise<void> | undefined,
}));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: async (
    command: string,
    args: { session?: Session; sessionId?: string },
  ) => {
    if (command === "session_get") {
      if (transport.failReads.has(args.sessionId!))
        throw new Error("store lookup failed");
      return transport.records.get(args.sessionId!) ?? null;
    }
    if (command === "session_delete") {
      transport.records.delete(args.sessionId!);
      return;
    }
    if (command !== "session_upsert")
      throw new Error(`Unexpected command ${command}`);
    const session = args.session!;
    transport.writes.push({
      id: session.id,
      blocks: structuredClone(session.blocks),
    });
    if (session.id.endsWith("parent")) await transport.waitParent;
    if (session.id.endsWith("parent") && transport.failParent)
      throw new Error("receipt disk failure");
    const record = { ...structuredClone(session), createdAt: 1, updatedAt: 2 };
    transport.records.set(session.id, record);
    return record;
  },
}));

beforeEach(() => {
  transport.records.clear();
  transport.writes.length = 0;
  transport.failParent = false;
  transport.failReads.clear();
  transport.waitParent = undefined;
});

describe("receipt repair through queued sessionStore writes and hydration", () => {
  async function seed(prefix: string) {
    const parentId = `${prefix}-parent`,
      childId = `${prefix}-child`;
    const reference = {
      kind: "linked" as const,
      parentId,
      childId,
      generation: 1,
      requestKey: `app-${parentId}-1`,
    };
    const parent = appendUser(
      { ...newSession("pi", "/tmp/project"), id: parentId },
      "Parent task",
    );
    const child = appendUser(
      { ...newSession("pi", "/tmp/project"), id: childId },
      "Only durable task copy",
      [],
      { appRequestId: reference.requestKey, acceptedAssignment: reference },
    );
    await upsertSession(parent);
    await upsertSession(child);
    return { parentId, childId, reference };
  }

  const storeHost: AssignmentReceiptHost = {
    session: async (id) => (await getSession(id)) ?? undefined,
    replace: (session) => session,
    persist: upsertSession,
  };

  it("repairs child-saved/parent-write-failed before edit so restart keeps the exact original assignment", async () => {
    const t = await seed("edit-restart");
    let child = (await getSession(t.childId))!;
    transport.failParent = true;
    expect(
      (await persistAcceptedAssignment(child, storeHost)).persistenceError,
    ).toContain("receipt disk failure");
    expect((await getSession(t.parentId))?.blocks).toHaveLength(1);
    transport.failParent = false;
    const attempt = createEditedResendAttempt(child)!;
    const rewind = vi.fn(() => {
      // The queued store write has completed before the destructive continuation.
      expect(transport.records.get(t.parentId)).toMatchObject({
        blocks: [
          { text: "Parent task" },
          {
            role: "system",
            assignmentReceipt: {
              ...t.reference,
              task: "Only durable task copy",
            },
          },
        ],
      });
      attempt.markProviderRewound();
      child = appendUser(attempt.replace(child), "Edited provider task");
      return true;
    });
    await expect(
      submitPreservedEditedResend(child, storeHost, rewind),
    ).resolves.toBe(true);
    await upsertSession(child);
    expect(rewind).toHaveBeenCalledOnce();
    // Restart using only real sessionStore hydration, not in-memory receipts.
    const restartedChild = (await getSession(t.childId))!;
    const restartedParent = (await getSession(t.parentId))!;
    expect(restartedChild.blocks.map((block) => block.text)).toEqual([
      "Edited provider task",
    ]);
    expect(restartedChild.blocks[0].acceptedAssignment).toBeUndefined();
    expect(restartedParent.blocks[1].assignmentReceipt).toMatchObject({
      ...t.reference,
      task: "Only durable task copy",
    });
    expect(restartedParent.blocks.map((block) => block.role)).toEqual([
      "user",
      "system",
    ]);
  });

  it.each(["save", "read"])(
    "refuses edit before rewind or truncation when receipt %s fails",
    async (failure) => {
      const t = await seed(`edit-${failure}-error`);
      let child = (await getSession(t.childId))!;
      const original = child;
      transport.failParent = failure === "save";
      if (failure === "read") transport.failReads.add(t.parentId);
      const attempt = createEditedResendAttempt(child)!;
      const rewind = vi.fn(() => {
        attempt.markProviderRewound();
        child = attempt.replace(child);
        return true;
      });
      await expect(
        submitPreservedEditedResend(child, storeHost, rewind),
      ).rejects.toThrow(
        failure === "save" ? "receipt disk failure" : "store lookup failed",
      );
      expect(rewind).not.toHaveBeenCalled();
      expect(child).toBe(original);
      expect((await getSession(t.childId))?.blocks[0]).toMatchObject({
        text: "Only durable task copy",
        acceptedAssignment: t.reference,
      });
      expect(attempt.recoverAfterFailure(child)).toBe(original);
    },
  );

  it("awaits a durable retry even when the failed receipt already exists in an open parent", async () => {
    const t = await seed("edit-memory");
    const child = (await getSession(t.childId))!;
    let parent = (await getSession(t.parentId))!;
    const host: AssignmentReceiptHost = {
      ...storeHost,
      session: async () => parent,
      replace: (next) => (parent = next),
    };
    transport.failParent = true;
    expect(
      (await persistAcceptedAssignment(child, host)).persistenceError,
    ).toContain("receipt disk failure");
    expect(parent.blocks[1].assignmentReceipt?.task).toBe(
      "Only durable task copy",
    );
    expect((await getSession(t.parentId))?.blocks).toHaveLength(1);
    transport.failParent = false;
    let release!: () => void;
    transport.waitParent = new Promise<void>((resolve) => {
      release = resolve;
    });
    const submit = vi.fn(() => true);
    const editing = submitPreservedEditedResend(child, host, submit);
    await vi.waitFor(() =>
      expect(
        transport.writes.filter((write) => write.id === t.parentId),
      ).toHaveLength(3),
    );
    expect(submit).not.toHaveBeenCalled();
    release();
    await expect(editing).resolves.toBe(true);
    expect(submit).toHaveBeenCalledOnce();
    expect(
      (await getSession(t.parentId))?.blocks[1].assignmentReceipt?.task,
    ).toBe("Only durable task copy");
    expect(parent.blocks).toHaveLength(2);
  });

  it("preserves every accepted block removed by a steered Codex edit", async () => {
    const t = await seed("edit-codex");
    const first = (await getSession(t.childId))!;
    const secondReference = {
      ...t.reference,
      generation: 2,
      requestKey: "app-codex-steer",
    };
    const child: Session = {
      ...first,
      harness: "codex",
      blocks: [
        { ...first.blocks[0], providerTurnId: "same-turn" },
        {
          id: "steer",
          role: "user",
          text: "Second exact task",
          providerTurnId: "same-turn",
          appRequestId: secondReference.requestKey,
          acceptedAssignment: secondReference,
        },
      ],
    };
    const attempt = createEditedResendAttempt(child)!;
    await submitPreservedEditedResend(child, storeHost, () => {
      expect(attempt.replace(child).blocks).toEqual([]);
      return true;
    });
    const parent = (await getSession(t.parentId))!;
    expect(
      parent.blocks.slice(1).map((block) => block.assignmentReceipt),
    ).toMatchObject([
      { ...t.reference, task: "Only durable task copy" },
      { ...secondReference, task: "Second exact task" },
    ]);
  });

  it("allows an edit when the original parent was authoritatively deleted", async () => {
    const t = await seed("edit-deleted");
    const child = (await getSession(t.childId))!;
    await deleteSession(t.parentId);
    const submit = vi.fn(() => true);
    await expect(
      submitPreservedEditedResend(child, storeHost, submit),
    ).resolves.toBe(true);
    expect(submit).toHaveBeenCalledOnce();
    expect(await getSession(t.parentId)).toBeNull();
  });

  it("the real deletion path preserves missing parent receipts and refuses deletion after save failure", async () => {
    const t = await seed("save-error");
    transport.failParent = true;
    await expect(deleteSession(t.childId)).rejects.toThrow(
      "receipt disk failure",
    );
    expect((await getSession(t.childId))?.blocks[0].acceptedAssignment).toEqual(
      t.reference,
    );
    transport.failParent = false;
    await deleteSession(t.childId);
    expect(await getSession(t.childId)).toBeNull();
    expect(
      (await getSession(t.parentId))?.blocks[1].assignmentReceipt?.task,
    ).toBe("Only durable task copy");
  });

  it("allows child deletion only after an authoritative lookup establishes the parent was intentionally deleted", async () => {
    const t = await seed("deleted");
    await deleteSession(t.parentId);
    expect(await getSession(t.parentId)).toBeNull();
    await deleteSession(t.childId);
    expect(await getSession(t.childId)).toBeNull();
  });

  it("a failed authoritative parent lookup refuses deletion and leaves the accepted task inspectable", async () => {
    const t = await seed("lookup-error");
    transport.failReads.add(t.parentId);
    await expect(deleteSession(t.childId)).rejects.toThrow(
      "store lookup failed",
    );
    expect((await getSession(t.childId))?.blocks[0].text).toBe(
      "Only durable task copy",
    );
  });
  it("recovers child-saved/parent-missing cut point with durable immutable provenance and no new user turn", async () => {
    const reference = {
      kind: "linked" as const,
      parentId: "durable-parent",
      childId: "durable-child",
      generation: 7,
      requestKey: "app-durable-parent-7",
      name: "Original name",
    };
    const parent = appendUser(
      { ...newSession("pi", "/tmp/project"), id: "durable-parent" },
      "Parent task",
    );
    const child = appendUser(
      { ...newSession("pi", "/tmp/project"), id: "durable-child" },
      "x".repeat(240_000),
      [],
      { appRequestId: reference.requestKey, acceptedAssignment: reference },
    );
    const open = new Map(
      [parent, child].map((session) => [session.id, session]),
    );
    const host: AssignmentReceiptHost = {
      session: async (id) =>
        open.get(id) ?? (await getSession(id)) ?? undefined,
      replace: (session) => {
        if (open.has(session.id)) open.set(session.id, session);
        return session;
      },
      persist: upsertSession,
    };
    await upsertSession(parent);
    transport.failParent = true;
    const result = await persistAcceptedAssignment(child, host);
    expect(result.persistenceError).toContain("Work was accepted");
    const childSnapshots = transport.writes.filter(
      (write) => write.id === "durable-child",
    );
    expect(childSnapshots).toHaveLength(1);
    expect(childSnapshots[0].blocks[0]).toMatchObject({
      appRequestId: reference.requestKey,
      acceptedAssignment: reference,
      text: "x".repeat(240_000),
    });
    // Restart: getSession is the real history consumer, with only SQLite transport doubled.
    open.clear();
    transport.failParent = false;
    const loadedChild = await getSession("durable-child");
    expect(loadedChild?.blocks[0].acceptedAssignment).toEqual(reference);
    await repairAssignmentReceipts(loadedChild!, host);
    const loadedParent = await getSession("durable-parent");
    expect(loadedParent?.blocks.map((block) => block.role)).toEqual([
      "user",
      "system",
    ]);
    expect(loadedParent?.blocks[1].assignmentReceipt).toMatchObject({
      ...reference,
      task: "x".repeat(240_000),
      harness: "pi",
    });
    await repairAssignmentReceipts(loadedChild!, host);
    expect((await getSession("durable-parent"))?.blocks).toHaveLength(2);
    expect(
      transport.writes.filter((write) => write.id === "durable-child"),
    ).toHaveLength(1);
  });
});
