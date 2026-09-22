import { describe, expect, it, vi } from "vitest";
import {
  canEditLastTurn,
  createEditedResendAttempt,
  createEditedResendCoordinator,
  lastEditableTurnStartIndex,
  lastTurnRecall,
  lastUserTurnStartIndex,
  prepareEditedResend,
  replaceEditedResend,
  truncateBeforeLastEditableTurn,
  truncateBeforeLastUserTurn,
} from "./editLastTurn";
import { newSession, type Block } from "./session";

function chat(blocks: Block[]) {
  return { ...newSession("pi", "/tmp"), blocks };
}

describe("editLastTurn", () => {
  it("finds the latest user turn", () => {
    const blocks: Block[] = [
      { id: "u1", role: "user", text: "first" },
      { id: "a1", role: "assistant", text: "ok" },
      { id: "u2", role: "user", text: "second" },
      { id: "a2", role: "assistant", text: "done" },
    ];
    expect(lastUserTurnStartIndex(blocks)).toBe(2);
    expect(truncateBeforeLastUserTurn(blocks).map((block) => block.id)).toEqual([
      "u1",
      "a1",
    ]);
  });

  it("ignores draft user blocks when selecting the editable turn", () => {
    const blocks: Block[] = [
      { id: "submitted", role: "user", text: "keep this" },
      { id: "reply", role: "assistant", text: "answer" },
      { id: "draft", role: "user", text: "saved draft", draft: true },
    ];
    const session = chat(blocks);

    expect(lastUserTurnStartIndex(blocks)).toBe(0);
    expect(lastTurnRecall(session)).toEqual({
      text: "keep this",
      attachments: [],
    });
    expect(canEditLastTurn(session)).toBe(true);
  });

  it("ignores internal user turns when selecting the editable turn", () => {
    const blocks: Block[] = [
      { id: "visible", role: "user", text: "keep this" },
      { id: "reply", role: "assistant", text: "answer" },
      {
        id: "internal",
        role: "user",
        text: "hidden orchestration prompt",
        internal: true,
      },
    ];
    const session = chat(blocks);

    expect(lastUserTurnStartIndex(blocks)).toBe(0);
    expect(truncateBeforeLastUserTurn(blocks)).toEqual([]);
    expect(lastTurnRecall(session)).toEqual({
      text: "keep this",
      attachments: [],
    });
    expect(canEditLastTurn(session)).toBe(true);
  });

  it("recalls the last user message", () => {
    const session = chat([
      { id: "u1", role: "user", text: "hello" },
      { id: "a1", role: "assistant", text: "hi" },
    ]);
    expect(lastTurnRecall(session)).toEqual({
      text: "hello",
      attachments: [],
    });
  });

  it("allows edit on idle pi sessions without queued follow-ups", () => {
    const session = chat([
      { id: "u1", role: "user", text: "hello" },
      { id: "a1", role: "assistant", text: "hi" },
    ]);
    expect(canEditLastTurn(session)).toBe(true);
  });

  it("rewinds the whole Codex turn when the last message was steered", () => {
    const session = {
      ...newSession("codex", "/tmp"),
      blocks: [
        { id: "u1", role: "user" as const, text: "first", providerTurnId: "t1" },
        { id: "a1", role: "assistant" as const, text: "done" },
        { id: "u2", role: "user" as const, text: "second", providerTurnId: "t2" },
        { id: "a2", role: "assistant" as const, text: "working" },
        {
          id: "u3",
          role: "user" as const,
          text: "focus on tests",
          providerTurnId: "t2",
        },
        { id: "a3", role: "assistant" as const, text: "updated" },
      ],
    };

    expect(lastEditableTurnStartIndex(session)).toBe(2);
    expect(truncateBeforeLastEditableTurn(session).map((block) => block.id)).toEqual([
      "u1",
      "a1",
    ]);
    expect(prepareEditedResend(session)).toMatchObject({
      providerTurnId: "t2",
      blocks: [
        { id: "u1" },
        { id: "a1" },
      ],
    });
    expect(replaceEditedResend(session).blocks.map((block) => block.id)).toEqual([
      "u1",
      "a1",
    ]);
  });

  it("restores edit mode when the provider never rewound", () => {
    const session = chat([
      { id: "u1", role: "user", text: "hello" },
      { id: "a1", role: "assistant", text: "hi" },
    ]);
    const rejected = vi.fn();
    const attempt = createEditedResendAttempt(session, rejected)!;

    expect(attempt.recoverAfterFailure(session)).toBe(session);
    attempt.reject();
    attempt.reject();

    expect(rejected).toHaveBeenCalledOnce();
    expect(rejected).toHaveBeenCalledWith({ providerRewound: false });
  });

  it("keeps provider and local history rewound when replacement is rejected", () => {
    const session = chat([
      { id: "u1", role: "user", text: "first" },
      { id: "a1", role: "assistant", text: "done" },
      { id: "u2", role: "user", text: "replace me" },
      { id: "a2", role: "assistant", text: "old answer" },
    ]);
    const rejected = vi.fn();
    const attempt = createEditedResendAttempt(session, rejected)!;

    attempt.markProviderRewound();
    const recovered = attempt.recoverAfterFailure(session);
    attempt.reject();

    expect(recovered.blocks.map((block) => block.id)).toEqual(["u1", "a1"]);
    expect(rejected).toHaveBeenCalledWith({ providerRewound: true });
  });

  it("does not reject or roll back a replacement accepted by the provider", () => {
    const session = chat([
      { id: "u1", role: "user", text: "hello" },
      { id: "a1", role: "assistant", text: "hi" },
    ]);
    const rejected = vi.fn();
    const attempt = createEditedResendAttempt(session, rejected)!;

    attempt.markProviderRewound();
    attempt.markAccepted();
    attempt.reject();

    expect(attempt.isAccepted()).toBe(true);
    expect(attempt.recoverAfterFailure(session)).toBe(session);
    expect(rejected).not.toHaveBeenCalled();
  });

  it("coordinates only one rewind per session", () => {
    const coordinator = createEditedResendCoordinator();

    expect(coordinator.start("one")).toBe(true);
    expect(coordinator.isActive("one")).toBe(true);
    expect(coordinator.start("one")).toBe(false);
    expect(coordinator.start("two")).toBe(true);
    coordinator.finish("one");
    expect(coordinator.isActive("one")).toBe(false);
    expect(coordinator.start("one")).toBe(true);
  });

  it("allows edit on idle OpenCode sessions", () => {
    const session = {
      ...chat([
        { id: "u1", role: "user", text: "hello" },
        { id: "a1", role: "assistant", text: "hi" },
      ]),
      harness: "opencode" as const,
    };
    expect(canEditLastTurn(session)).toBe(true);
  });

  it("blocks edit while busy, queued, or on unsupported harnesses", () => {
    const base = chat([
      { id: "u1", role: "user", text: "hello" },
      { id: "a1", role: "assistant", text: "hi" },
    ]);
    expect(canEditLastTurn({ ...base, busy: true })).toBe(false);
    expect(
      canEditLastTurn({
        ...base,
        queuedMessages: [
          { id: "q1", text: "next", attachments: [] },
        ],
      }),
    ).toBe(false);
    expect(canEditLastTurn({ ...chat([]), harness: "claude" })).toBe(false);
  });
});
