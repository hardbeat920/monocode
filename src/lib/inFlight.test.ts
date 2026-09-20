import { describe, expect, it } from "vitest";
import { newTab } from "./layout";
import {
  CONTINUE_PROMPT,
  INTERRUPT_MESSAGE,
  canAutoContinue,
  hasInFlightSessions,
  inFlightRefs,
  isInFlightSession,
  markTurnInterrupted,
  quitWhileBusyMessage,
  shouldWriteInFlightSnapshot,
  workspaceFromResumed,
} from "./inFlight";
import { newSession, type Session } from "./session";

function chat(
  cwd: string,
  patch: Partial<Session> = {},
): Session {
  const session = newSession("cursor", cwd);
  session.blocks = [{ id: "u1", role: "user", text: "hello" }];
  return { ...session, ...patch, blocks: patch.blocks ?? session.blocks };
}

describe("isInFlightSession", () => {
  it("is true for a busy turn, a live approval, or a parked question", () => {
    expect(isInFlightSession(chat("/tmp/a"))).toBe(false);
    expect(isInFlightSession(chat("/tmp/a", { busy: true }))).toBe(true);
    expect(
      isInFlightSession(
        chat("/tmp/a", {
          blocks: [
            { id: "u1", role: "user", text: "hello" },
            {
              id: "a1",
              role: "approval",
              text: "run rm",
              approval: { requestId: 1 },
            },
          ],
        }),
      ),
    ).toBe(true);
    expect(
      isInFlightSession(
        chat("/tmp/a", {
          pendingQuestion: {
            requestId: 4,
            questions: [
              {
                id: "q1",
                prompt: "Which file?",
                multiSelect: false,
                allowCustom: true,
                options: [{ id: "a.ts", label: "a.ts" }],
              },
            ],
          },
        }),
      ),
    ).toBe(true);
  });
});

describe("inFlightRefs", () => {
  it("walks open tabs first, then parked busy sessions", () => {
    const parked = chat("/tmp/parked", { busy: true });
    const openBusy = chat("/tmp/open", { busy: true });
    const idle = chat("/tmp/idle");
    const blank = newSession("cursor", "/tmp/blank");
    blank.busy = true;

    const tabs = [newTab(idle.id), newTab(openBusy.id)];
    expect(
      inFlightRefs([parked, openBusy, idle, blank], tabs),
    ).toEqual([
      { sessionId: openBusy.id, cwd: "/tmp/open" },
      { sessionId: parked.id, cwd: "/tmp/parked" },
    ]);
  });

  it("skips chats that were never persisted", () => {
    const blank = newSession("cursor", "/tmp/a");
    blank.busy = true;
    expect(inFlightRefs([blank], [newTab(blank.id)])).toEqual([]);
    expect(hasInFlightSessions([blank])).toBe(true);
  });

  it("does not resume a session whose worktree was removed", () => {
    const removed = chat("/tmp/a", {
      busy: true,
      worktreeCwd: "/tmp/a-worktrees/feature",
      worktreeRemoved: true,
    });
    expect(isInFlightSession(removed)).toBe(false);
    expect(inFlightRefs([removed], [newTab(removed.id)])).toEqual([]);
  });
});

describe("markTurnInterrupted", () => {
  it("seals the stream, cancels open tools, and appends a system note", () => {
    const interrupted = markTurnInterrupted(
      chat("/tmp/a", {
        busy: true,
        blocks: [
          { id: "u1", role: "user", text: "hello", startedAt: 1_000 },
          { id: "a1", role: "assistant", text: "Working", streaming: true },
          {
            id: "t1",
            role: "tool",
            text: "edit",
            streaming: true,
            tool: { status: "running", title: "edit" },
          },
          {
            id: "p1",
            role: "approval",
            text: "allow?",
            approval: { requestId: 7 },
          },
        ],
      }),
    );

    expect(interrupted.busy).toBe(false);
    expect(interrupted.blocks).toEqual([
      expect.objectContaining({
        id: "u1",
        durationMs: expect.any(Number),
      }),
      expect.objectContaining({
        id: "a1",
        streaming: false,
        text: "Working",
      }),
      expect.objectContaining({
        id: "t1",
        streaming: false,
        tool: expect.objectContaining({ status: "cancelled" }),
      }),
      expect.objectContaining({
        role: "system",
        text: INTERRUPT_MESSAGE,
      }),
    ]);
  });

  it("does not append the interrupt note twice for the same turn", () => {
    const once = markTurnInterrupted(chat("/tmp/a", { busy: true }));
    const twice = markTurnInterrupted(once);
    expect(
      twice.blocks.filter((block) => block.text === INTERRUPT_MESSAGE),
    ).toHaveLength(1);
  });

  it("appends a new note when a later turn is quit after Continue", () => {
    const first = markTurnInterrupted(
      chat("/tmp/a", { busy: true, providerSessionId: "p1" }),
    );
    const continued: Session = {
      ...first,
      blocks: [
        ...first.blocks,
        { id: "c1", role: "user", text: CONTINUE_PROMPT },
        { id: "a2", role: "assistant", text: "resumed" },
        { id: "u2", role: "user", text: "edit the readme" },
        {
          id: "t2",
          role: "tool",
          text: "read",
          tool: { status: "completed", title: "read" },
        },
      ],
      busy: true,
    };
    const second = markTurnInterrupted(continued);
    expect(
      second.blocks.filter((block) => block.text === INTERRUPT_MESSAGE),
    ).toHaveLength(2);
    expect(second.blocks[second.blocks.length - 1]?.text).toBe(INTERRUPT_MESSAGE);
    expect(canAutoContinue(second)).toBe(true);
  });

  it("leaves finished tools alone", () => {
    const interrupted = markTurnInterrupted(
      chat("/tmp/a", {
        busy: true,
        blocks: [
          { id: "u1", role: "user", text: "hello" },
          {
            id: "t1",
            role: "tool",
            text: "read",
            tool: { status: "completed", title: "read" },
          },
        ],
      }),
    );
    expect(interrupted.blocks[1]?.tool?.status).toBe("completed");
  });
});

describe("quitWhileBusyMessage", () => {
  it("mentions resume on reopen", () => {
    expect(quitWhileBusyMessage(1)).toContain("1 chat is still running");
    expect(quitWhileBusyMessage(3)).toContain("3 chats are still running");
  });
});

describe("workspaceFromResumed", () => {
  it("opens one tab per restored chat", () => {
    const first = chat("/tmp/a");
    const second = chat("/tmp/b");
    const workspace = workspaceFromResumed([first, second]);
    expect(workspace?.projectCwd).toBe("/tmp/a");
    expect(workspace?.sessions.map((session) => session.id)).toEqual([
      first.id,
      second.id,
    ]);
    expect(workspace?.tabs).toHaveLength(2);
    expect(workspace?.tabs[0]?.focusedId).toBe(first.id);
    expect(workspace?.activeTabId).toBe(workspace?.tabs[0]?.id);
  });

  it("returns null when there is nothing to restore", () => {
    expect(workspaceFromResumed([])).toBeNull();
  });
});

describe("canAutoContinue", () => {
  it("needs a provider thread and an interrupt note as the last block", () => {
    const interrupted = markTurnInterrupted(
      chat("/tmp/a", { busy: true, providerSessionId: "p1" }),
    );
    expect(canAutoContinue(interrupted)).toBe(true);
    expect(canAutoContinue(chat("/tmp/a", { providerSessionId: "p1" }))).toBe(
      false,
    );
    expect(canAutoContinue(markTurnInterrupted(chat("/tmp/a", { busy: true })))).toBe(
      false,
    );
    expect(
      canAutoContinue({
        ...interrupted,
        blocks: [
          ...interrupted.blocks,
          { id: "c1", role: "user", text: "Continue from where you left off." },
        ],
      }),
    ).toBe(false);
    expect(canAutoContinue({ ...interrupted, busy: true })).toBe(false);
    expect(canAutoContinue({ ...interrupted, worktreeRemoved: true })).toBe(
      false,
    );
  });
});

describe("shouldWriteInFlightSnapshot", () => {
  it("does not wipe a disk snapshot on the first idle paint", () => {
    expect(shouldWriteInFlightSnapshot("", [], null, false)).toBe(false);
  });

  it("writes when a chat becomes in-flight", () => {
    expect(
      shouldWriteInFlightSnapshot("a", [{ sessionId: "a", cwd: "/tmp" }], null, false),
    ).toBe(true);
  });

  it("clears after this process has seen an in-flight chat", () => {
    expect(shouldWriteInFlightSnapshot("", [], "a", true)).toBe(true);
  });

  it("skips unchanged keys", () => {
    expect(
      shouldWriteInFlightSnapshot("a", [{ sessionId: "a", cwd: "/tmp" }], "a", true),
    ).toBe(false);
  });
});
