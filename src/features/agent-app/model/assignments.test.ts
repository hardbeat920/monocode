// @vitest-environment happy-dom
import { describe, expect, it } from "vitest";
import {
  appendUser,
  applyHarnessEvent,
} from "../../../integrations/harness/core/apply";
import { newSession, type Session } from "../../sessions/model/session";
import { sanitizeSessionForPersist } from "../../sessions/data/sessionStore";
import { buildDeterministicHandoff } from "../../sessions/model/handoff";
import { sessionConversationPage } from "./sessionConversation";
import {
  persistAcceptedAssignment,
  repairAssignmentReceipts,
  type AssignmentReceiptHost,
  type AssignmentReference,
} from "./assignments";

const reference: AssignmentReference = {
  kind: "linked",
  parentId: "parent",
  childId: "child",
  generation: 1,
  requestKey: "app-parent-1",
  name: "Reviewer",
};
function fixture() {
  const parent = { ...newSession("codex", "/tmp/project"), id: "parent" };
  const child = appendUser(
    { ...newSession("pi", "/tmp/project"), id: "child" },
    "Exact task\n<xml> & details",
    [],
    {
      appRequestId: reference.requestKey,
      acceptedAssignment: reference,
    },
  );
  const open = new Map<string, Session>([
    [parent.id, parent],
    [child.id, child],
  ]);
  const disk = new Map<string, Session>();
  const writes: string[] = [];
  const state = { failParent: false, failChild: false };
  const host: AssignmentReceiptHost = {
    session: async (id) => open.get(id) ?? disk.get(id),
    replace: (session) => {
      open.set(session.id, session);
      return session;
    },
    persist: async (session) => {
      writes.push(session.id);
      if (
        (state.failParent && session.id === "parent") ||
        (state.failChild && session.id === "child")
      )
        throw new Error("disk full");
      const persisted = sanitizeSessionForPersist(session);
      disk.set(session.id, structuredClone(persisted));
      return persisted;
    },
  };
  return { parent, child, open, disk, state, writes, host };
}

describe("durable accepted assignment receipts", () => {
  it("persists provenance with the actual child user block before its display-only parent receipt", async () => {
    const t = fixture();
    expect(await persistAcceptedAssignment(t.child, t.host)).toEqual({});
    expect(t.writes).toEqual(["child", "parent"]);
    expect(t.disk.get("child")?.blocks[0]).toMatchObject({
      role: "user",
      text: "Exact task\n<xml> & details",
      appRequestId: "app-parent-1",
      acceptedAssignment: reference,
    });
    expect(t.disk.get("parent")?.blocks[0]).toMatchObject({
      role: "system",
      text: "",
      assignmentReceipt: { ...reference, task: t.child.blocks[0].text },
    });
    await persistAcceptedAssignment(t.child, t.host);
    expect(t.open.get("parent")?.blocks).toHaveLength(1);
  });

  it("keeps acceptance after failed parent save, repairs after restart without resubmission or generation change", async () => {
    const t = fixture();
    t.state.failParent = true;
    const accepted = await persistAcceptedAssignment(t.child, t.host);
    expect(accepted.persistenceError).toContain("Work was accepted");
    expect(t.disk.get("child")?.blocks[0].acceptedAssignment).toEqual(
      reference,
    );
    expect(t.disk.has("parent")).toBe(false);
    // Restart cut point: accepted child is durable, receipt is absent on disk.
    t.open.clear();
    t.disk.set("parent", t.parent);
    t.state.failParent = false;
    const savedChild = t.disk.get("child")!;
    await repairAssignmentReceipts(savedChild, t.host);
    expect(t.disk.get("parent")?.blocks[0].assignmentReceipt).toMatchObject({
      ...reference,
      task: t.child.blocks[0].text,
      harness: "pi",
    });
    expect(t.disk.get("child")?.blocks).toHaveLength(1);
    expect(t.disk.get("child")?.blocks[0].acceptedAssignment?.kind).toBe(
      "linked",
    );
    expect(t.writes).toEqual(["child", "parent", "parent"]);
  });

  it("retries an in-memory receipt whose write failed and refuses deletion preservation on another failure", async () => {
    const t = fixture();
    t.state.failParent = true;
    await persistAcceptedAssignment(t.child, t.host);
    await expect(repairAssignmentReceipts(t.child, t.host)).rejects.toThrow(
      "disk full",
    );
    expect(t.open.get("parent")?.blocks).toHaveLength(1);
    t.state.failParent = false;
    expect(await persistAcceptedAssignment(t.child, t.host)).toEqual({});
    expect(t.disk.get("parent")?.blocks).toHaveLength(1);
  });

  it("permits deletion/reconciliation after authoritative parent deletion, but not failed lookups or writes", async () => {
    const t = fixture();
    t.open.delete("parent");
    await expect(
      repairAssignmentReceipts(t.child, t.host, { skipDeletedParents: true }),
    ).resolves.toBeUndefined();
    expect(t.writes).toEqual([]);
    const failingRead: AssignmentReceiptHost = {
      ...t.host,
      session: async () => {
        throw new Error("store read failed");
      },
    };
    await expect(
      repairAssignmentReceipts(t.child, failingRead, {
        skipDeletedParents: true,
      }),
    ).rejects.toThrow("store read failed");
    t.open.set("parent", t.parent);
    t.state.failParent = true;
    await expect(
      repairAssignmentReceipts(t.child, t.host, { skipDeletedParents: true }),
    ).rejects.toThrow("disk full");
    expect(t.open.get("child")?.blocks[0].acceptedAssignment).toEqual(
      reference,
    );
  });

  it("a new acceptance repairs its own receipt even when an older owner was deleted", async () => {
    const t = fixture();
    const previous = {
      ...reference,
      parentId: "deleted-parent",
      requestKey: "app-deleted-parent-1",
    };
    const child = appendUser(
      {
        ...t.child,
        blocks: [
          {
            ...t.child.blocks[0],
            appRequestId: previous.requestKey,
            acceptedAssignment: previous,
          },
        ],
      },
      "New task",
      [],
      { appRequestId: reference.requestKey, acceptedAssignment: reference },
    );
    expect(
      await persistAcceptedAssignment(child, t.host, reference.requestKey),
    ).toEqual({});
    expect(t.disk.get("parent")?.blocks[0].assignmentReceipt?.task).toBe(
      "New task",
    );
  });

  it("surfaces an accepted child write failure without discarding repair evidence", async () => {
    const t = fixture();
    t.state.failChild = true;
    expect(
      (await persistAcceptedAssignment(t.child, t.host)).persistenceError,
    ).toContain("disk full");
    expect(t.open.get("child")?.blocks[0].acceptedAssignment).toEqual(
      reference,
    );
    t.state.failChild = false;
    await persistAcceptedAssignment(t.child, t.host);
    expect(t.disk.get("parent")?.blocks[0].assignmentReceipt?.task).toBe(
      t.child.blocks[0].text,
    );
  });

  it("does not invent acceptance from titles, user text or drafts", async () => {
    const t = fixture();
    await repairAssignmentReceipts(
      {
        ...t.child,
        title: "Reviewer",
        blocks: [
          {
            id: "draft",
            role: "user",
            text: "task",
            appRequestId: reference.requestKey,
            acceptedAssignment: reference,
            draft: true,
          },
        ],
      },
      t.host,
    );
    await repairAssignmentReceipts(
      { ...t.child, blocks: [{ id: "legacy", role: "user", text: "task" }] },
      t.host,
    );
    expect(t.writes).toEqual([]);
  });

  it("records ancestor messages without owned generation", async () => {
    const t = fixture();
    const message = {
      kind: "message" as const,
      parentId: "parent",
      childId: "child",
      requestKey: "app-parent-2",
    };
    const child = appendUser(t.child, "Message to ancestor", [], {
      appRequestId: message.requestKey,
      acceptedAssignment: message,
    });
    await persistAcceptedAssignment(child, t.host);
    const receipt = t.disk.get("parent")?.blocks[1].assignmentReceipt;
    expect(receipt).toMatchObject({
      ...message,
      task: "Message to ancestor",
      harness: "pi",
    });
    expect(receipt).not.toHaveProperty("generation");
  });

  it("parent receipts cannot steal active turn metrics, exchanges or handoff goals", async () => {
    const t = fixture();
    const active = appendUser(t.parent, "Parent's actual task");
    t.open.set("parent", active);
    const before = buildDeterministicHandoff(active);
    await persistAcceptedAssignment(t.child, t.host);
    const updated = applyHarnessEvent(t.open.get("parent")!, {
      type: "turn.metrics",
      inputTokens: 71,
      outputTokens: 3,
    });
    expect(updated.blocks[0].turnMetrics).toMatchObject({
      inputTokens: 71,
      outputTokens: 3,
    });
    expect(updated.blocks[1].turnMetrics).toBeUndefined();
    expect(
      sessionConversationPage(updated).turns.map((turn) => turn.user.text),
    ).toEqual(["Parent's actual task"]);
    expect(buildDeterministicHandoff(updated)).toBe(before);
  });
});
