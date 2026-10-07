import { describe, expect, it } from "vitest";
import { applyHarnessEvent } from "../../../integrations/harness/core/apply";
import { canDispatchQueuedHead } from "../../sessions/model/messageQueue";
import { CONTINUE_PROMPT } from "../../sessions/model/inFlight";
import {
  pendingComposerSwitch,
  planComposerSwitch,
  shouldAskOutgoingAgent,
} from "../../sessions/model/handoff";
import { newSession, type Session } from "../../sessions/model/session";
import {
  resumeMonoUsageLimit,
  switchMonoUsageLimitAccount,
} from "./monoUsageLimit";

function limited(patch: Partial<Session> = {}): Session {
  return {
    ...newSession("claude", "/tmp/project"),
    providerSessionId: "old-thread",
    providerAccountId: "default",
    blocks: [
      { id: "request", role: "user", text: "Fix the tests" },
      {
        id: "edit",
        role: "tool",
        text: "",
        tool: { kind: "edit", title: "Edit tests.ts" },
      },
    ],
    busy: false,
    usageLimit: { resetsAt: 1000, resumeAtReset: true },
    ...patch,
  };
}

describe("Mono usage recovery", () => {
  it("continues the stopped work once when there are no queued messages", () => {
    const original = limited();
    const resumed = resumeMonoUsageLimit(original);
    expect(resumed.usageLimit).toBeUndefined();
    expect(resumed.queuedMessages).toHaveLength(1);
    expect(resumed.queuedMessages?.[0].text).toBe(CONTINUE_PROMPT);
    expect(resumed.blocks.slice(0, 2)).toEqual(original.blocks);
    expect(canDispatchQueuedHead(resumed)).toBe(true);
    expect(resumeMonoUsageLimit(resumed)).toBe(resumed);
  });

  it("resumes the existing outbox without adding a duplicate continuation", () => {
    const attachments = [
      {
        id: "file",
        name: "tests.ts",
        kind: "file" as const,
        path: "/tmp/tests.ts",
        size: 10,
        mimeType: "text/plain",
      },
    ];
    const original = limited({
      queueStatus: "paused",
      queuedMessages: [
        {
          id: "first",
          blockId: "first",
          text: "Check this file",
          attachments,
          error: "Quota exceeded",
        },
        { id: "second", text: "Then run tests", attachments: [] },
      ],
    });
    const resumed = resumeMonoUsageLimit(original);
    expect(resumed.blocks).toBe(original.blocks);
    expect(resumed.queuedMessages?.map((message) => message.id)).toEqual([
      "first",
      "second",
    ]);
    expect(resumed.queuedMessages?.[0].attachments).toBe(attachments);
    expect(resumed.queuedMessages?.[0].error).toBeUndefined();
    expect(canDispatchQueuedHead(resumed)).toBe(true);
  });

  it("keeps busy or ordinary conversations unchanged", () => {
    const busy = limited({ busy: true });
    const ordinary = limited({ usageLimit: undefined });
    expect(resumeMonoUsageLimit(busy)).toBe(busy);
    expect(resumeMonoUsageLimit(ordinary)).toBe(ordinary);
  });

  it("switches providers with a transcript recap instead of contacting the exhausted provider", () => {
    const original = limited();
    const plan = planComposerSwitch(original, "codex");
    expect(plan.kind).toBe("arm");
    if (plan.kind !== "arm") throw new Error("Expected a handoff");
    const resumed = resumeMonoUsageLimit({
      ...original,
      harness: "codex",
      providerAccountId: undefined,
      providerSessionId: undefined,
      pendingSwitch: plan.pending,
    });
    expect(pendingComposerSwitch(resumed)?.from).toBe("claude");
    expect(shouldAskOutgoingAgent(resumed)).toBe(false);
    expect(canDispatchQueuedHead(resumed)).toBe(true);
  });

  it("uses a fresh account-owned thread while preserving the conversation", () => {
    const original = limited();
    const resumed = switchMonoUsageLimitAccount(original, "account-work");
    expect(resumed.providerAccountId).toBe("account-work");
    expect(resumed.providerSessionId).toBeUndefined();
    expect(resumed.blocks.slice(0, 2)).toEqual(original.blocks);
    expect(pendingComposerSwitch(resumed)).toMatchObject({
      from: "claude",
      fromProviderSessionId: "old-thread",
      fromProviderAccountId: "default",
      skipOutgoingRecap: true,
    });
    expect(shouldAskOutgoingAgent(resumed)).toBe(false);
    expect(canDispatchQueuedHead(resumed)).toBe(true);
    expect(switchMonoUsageLimitAccount(original, "default")).toBe(original);
    const legacy = limited({ providerAccountId: undefined });
    expect(switchMonoUsageLimitAccount(legacy, "default")).toBe(legacy);
  });

  it("shows recovery again if the replacement account also runs out", () => {
    const resumed = resumeMonoUsageLimit(limited());
    const stopped = applyHarnessEvent(resumed, {
      type: "session.error",
      message: "Quota exceeded",
    });
    expect(stopped.usageLimit).toEqual({});
    expect(stopped.queueStatus).toBe("paused");
    expect(canDispatchQueuedHead(stopped)).toBe(false);
  });
});
