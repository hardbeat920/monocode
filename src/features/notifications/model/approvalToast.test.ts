import { describe, expect, it } from "vitest";
import { newTab, splitPane } from "../../workspace/model/layout";
import {
  hiddenApprovalNotices,
  isSessionConversationFocused,
  pendingApprovalForSession,
} from "./approvalToast";
import { newSession, type Block } from "../../sessions/model/session";

function block(role: Block["role"], approval?: Block["approval"]): Block {
  return {
    id: crypto.randomUUID(),
    role,
    text: role === "tool" ? "Run command" : "Approve?",
    ...(role === "tool"
      ? { tool: { title: "Run command", kind: "execute" } }
      : {}),
    ...(approval ? { approval } : {}),
  };
}

describe("pendingApprovalForSession", () => {
  it("never toasts a worker approval, which its lead answers instead", () => {
    const worker = {
      ...newSession(),
      id: "worker",
      orchestrationLeadId: "lead",
      blocks: [block("tool", { requestId: 42 })],
    };
    const leadTab = newTab("lead");
    const otherTab = newTab("unrelated");
    const tabs = [leadTab, otherTab];
    for (const active of [leadTab, otherTab])
      expect(hiddenApprovalNotices([worker], active.id, tabs, true)).toEqual([]);
    // The same approval on an ordinary session still reaches the user.
    const solo = { ...worker, orchestrationLeadId: undefined };
    expect(
      hiddenApprovalNotices([solo], otherTab.id, tabs, true)[0].sessionId,
    ).toBe("worker");
  });
  it("returns the latest undecided approval", () => {
    const session = newSession();
    session.blocks = [
      block("tool", { requestId: 1, decided: "allow" }),
      block("tool", { requestId: 2 }),
    ];
    const pending = pendingApprovalForSession(session);
    expect(pending?.requestId).toBe(2);
    expect(pending?.label).toBe("Shell");
    expect(pending?.kind).toBe("approval");
  });

  it("prefers a parked clarifying question over a tool approval", () => {
    const session = newSession();
    session.blocks = [block("tool", { requestId: 2 })];
    session.pendingQuestion = {
      requestId: 9,
      title: "Which file?",
      questions: [
        {
          id: "q1",
          prompt: "Which file?",
          multiSelect: false,
          allowCustom: true,
          options: [{ id: "a.ts", label: "a.ts" }],
        },
      ],
    };
    const pending = pendingApprovalForSession(session);
    expect(pending?.requestId).toBe(9);
    expect(pending?.kind).toBe("question");
    expect(pending?.label).toBe("Which file?");
  });
});

describe("isSessionConversationFocused", () => {
  it("is true only when the session pane is focused on the active tab", () => {
    const session = newSession();
    const tab = newTab(session.id);
    expect(isSessionConversationFocused(session.id, tab.id, [tab], true)).toBe(
      true,
    );
    expect(isSessionConversationFocused(session.id, tab.id, [tab], false)).toBe(
      false,
    );
    expect(isSessionConversationFocused("other", tab.id, [tab], true)).toBe(
      false,
    );
  });
});

describe("hiddenApprovalNotices", () => {
  it("omits approvals that are already in the focused conversation", () => {
    const visible = newSession();
    visible.blocks = [block("tool", { requestId: 1 })];
    const hidden = newSession();
    hidden.blocks = [block("tool", { requestId: 2 })];

    const visibleTab = newTab(visible.id);
    const hiddenTab = newTab(hidden.id);

    expect(
      hiddenApprovalNotices(
        [visible, hidden],
        visibleTab.id,
        [visibleTab, hiddenTab],
        true,
      ).map((notice) => notice.sessionId),
    ).toEqual([hidden.id]);
  });

  it("shows approvals in another pane on the same tab", () => {
    const left = newSession();
    left.blocks = [block("tool", { requestId: 1 })];
    const right = newSession();
    const tab = {
      ...newTab(left.id),
      layout: splitPane(newTab(left.id).layout, left.id, "right", right.id),
      focusedId: right.id,
    };

    expect(
      hiddenApprovalNotices([left, right], tab.id, [tab], true).map(
        (notice) => notice.sessionId,
      ),
    ).toEqual([left.id]);
  });
});
