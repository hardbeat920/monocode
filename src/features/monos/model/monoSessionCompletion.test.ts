import { describe, expect, it } from "vitest";
import { submitWithSettlement } from "../../../app/model/managedSubmission";
import type { ControlOutcome } from "../../orchestration/model/orchestration";
import {
  canDispatchQueuedHead,
  canSteerQueuedHead,
  dequeueQueuedMessage,
  queuedMessageForSubmit,
} from "../../sessions/model/messageQueue";
import {
  newSession,
  type QueuedMessage,
  type Session,
} from "../../sessions/model/session";
import {
  dismissQueuedMonoSessionCompletion,
  enqueueMonoSessionCompletion,
  monoSessionCompletionMessage,
  monoSessionCompletionResult,
  MonoSessionCompletionBatches,
  parseWorkerHandback,
  wrapDelegatedWorkerPrompt,
} from "./monoSessionCompletion";

const options = {
  requestId: "app-mono-request",
  sessionId: "worker",
  project: "/code/project",
  prompt: "Fix the API and run tests",
  outcome: {
    status: "completed",
    text: "The API is fixed and tests pass.",
  } as ControlOutcome,
};

function groupedMessage() {
  let message!: QueuedMessage;
  const batches = new MonoSessionCompletionBatches((_, ready) => {
    message = ready;
  });
  for (const [sessionId, status] of [
    ["stopped", "cancelled"],
    ["failed", "failed"],
    ["completed", "completed"],
  ] as const) {
    batches.watch(
      { monoId: "mono", turn: 1 },
      sessionId,
    )(
      monoSessionCompletionResult({
        ...options,
        sessionId,
        requestId: sessionId,
        outcome: { status, text: `Result for ${sessionId}` },
      }),
    );
  }
  batches.closeInactive(() => false);
  return message;
}

describe("Mono session completion notifications", () => {
  it("removes only the appended handoff envelope and preserves marker-like user text", () => {
    const marker = "monocode_delegated_worker_handoff_v1";
    const originalPrompt =
      `Keep this literal marker: <${marker}>user supplied</${marker}>`;
    const finalText = [
      "Handoff status: partial",
      "## Summary",
      "Implemented the parser.",
      "## Changes or artifacts",
      "monoSessionCompletion.ts",
      "## Checks actually run",
      "Focused tests passed.",
      "## Open items or required input",
      "None.",
    ].join("\n");
    const result = monoSessionCompletionResult({
      ...options,
      prompt: wrapDelegatedWorkerPrompt(originalPrompt),
      outcome: { status: "failed", text: finalText },
    });

    expect(result.originalPrompt).toBe(originalPrompt);
    expect(
      monoSessionCompletionResult({ ...options, prompt: originalPrompt })
        .originalPrompt,
    ).toBe(originalPrompt);
    expect(result.result).toBe(finalText);
    expect(result.status).toBe("failed");
    expect(result.disposition).toBe("partial");
    expect(result.dispositionSource).toBe("valid_final_status_line");
    expect(result.handback).toEqual({
      disposition: "partial",
      summary: "Implemented the parser.",
      changesOrArtifacts: "monoSessionCompletion.ts",
      checksActuallyRun: "Focused tests passed.",
      unresolvedOrNeeded: "None.",
    });
  });

  it.each([
    ["Handoff status: completed", "completed"],
    ["Handoff status: partial", "partial"],
    ["Handoff status: blocked", "blocked"],
    ["Handoff status: needs user input", "needs_user_input"],
  ] as const)("parses the exact first-line disposition %s", (text, disposition) => {
    const parsed = parseWorkerHandback(
      `${text}\n## Summary\nWork report`,
      "cancelled",
    );
    expect(parsed.disposition).toBe(disposition);
    expect(parsed.source).toBe("valid_final_status_line");
    expect(parsed.runtimeStatus).toBe("cancelled");
    expect(parsed.finalAssistantText).toContain(text);
  });

  it.each([
    "",
    "Work completed\nHandoff status: completed",
    "Handoff status: COMPLETE",
    "Handoff status: completed with caveats",
    "**Handoff status: completed**",
  ])("keeps a missing or malformed first-line disposition unreported: %s", (text) => {
    const parsed = parseWorkerHandback(text, "failed");
    expect(parsed.disposition).toBe("unreported");
    expect(parsed.source).toBe("missing_or_malformed");
    expect(parsed.runtimeStatus).toBe("failed");
  });

  it("bounds structured handback fields alongside the final response", () => {
    const finalText = [
      "Handoff status: completed",
      "## Summary",
      "s".repeat(15_000),
      "## Changes or artifacts",
      "c".repeat(15_000),
      "## Checks actually run",
      "t".repeat(15_000),
      "## Open items or required input",
      "None.",
    ].join("\n");
    const message = monoSessionCompletionMessage({
      ...options,
      outcome: { status: "completed", text: finalText },
    });
    const payload = JSON.parse(
      message.text.slice(message.text.indexOf("\n\n") + 2),
    );
    const handback = payload.handback as {
      summary: string;
      changesOrArtifacts: string;
      checksActuallyRun: string;
      unresolvedOrNeeded: string;
    };

    expect(payload.truncated).toBe(true);
    expect(payload.result).toHaveLength(12_000);
    expect(
      handback.summary.length +
        handback.changesOrArtifacts.length +
        handback.checksActuallyRun.length +
        handback.unresolvedOrNeeded.length,
    ).toBeLessThanOrEqual(12_000);
    expect(message.text.length).toBeLessThan(30_000);
  });

  it("accepts immediately, then queues completion behind chat without steering a busy Mono", async () => {
    let mono: Session = {
      ...newSession("codex", "/code/project"),
      busy: true,
      turnReady: true,
      queuedMessages: [
        { id: "chat", text: "Also explain the design", attachments: [] },
      ],
    };
    let settle!: (outcome: ControlOutcome) => void;
    await expect(
      submitWithSettlement({
        submit: (done) => {
          settle = done;
          return true;
        },
        onSettled: (outcome) => {
          mono = enqueueMonoSessionCompletion(
            mono,
            monoSessionCompletionMessage({ ...options, outcome }),
          );
        },
        rejectionMessage: "Could not start",
      }),
    ).resolves.toBe(true);
    expect(mono.queuedMessages).toHaveLength(1);
    settle(options.outcome);
    settle(options.outcome);
    expect(mono.queuedMessages?.map((message) => message.id)).toEqual([
      "chat",
      "mono-completion-app-mono-request",
    ]);
    const next = dequeueQueuedMessage(mono, "chat");
    expect(canSteerQueuedHead(next)).toBe(false);
    expect(
      queuedMessageForSubmit(next, "mono-completion-app-mono-request", "steer"),
    ).toBeUndefined();
    expect(canDispatchQueuedHead(next)).toBe(false);
    expect(canDispatchQueuedHead({ ...next, busy: false })).toBe(true);
  });

  it("uses the final answer from the watched turn even after a later turn begins", () => {
    const session = newSession("codex", options.project);
    session.title = "API fix";
    session.blocks = [
      {
        id: "u",
        role: "user",
        text: options.prompt,
        appRequestId: options.requestId,
      },
      { id: "progress", role: "assistant", text: "I am checking" },
      {
        id: "tool",
        role: "tool",
        text: "private tool output",
        tool: { kind: "shell" },
      },
      {
        id: "reply",
        role: "assistant",
        text: "Fixed the endpoint; 12 tests pass.",
      },
      { id: "later", role: "user", text: "New task" },
      { id: "later-reply", role: "assistant", text: "Unrelated answer" },
    ];
    const message = monoSessionCompletionMessage({ ...options, session });
    expect(message.text).toContain("Fixed the endpoint; 12 tests pass.");
    expect(message.text).toContain(options.project);
    expect(message.text).toContain(options.prompt);
    expect(message.text).not.toContain("Unrelated answer");
    expect(message.text).not.toContain("private tool output");
    expect(message.monoSessionCompletion).toEqual({
      sessionId: "worker",
      title: "API fix",
      status: "completed",
    });
  });

  it.each(["failed", "cancelled"] as const)(
    "reports %s with available output and an error",
    (status) => {
      const message = monoSessionCompletionMessage({
        ...options,
        outcome: { status, text: "Partial work", error: "Agent stopped" },
      });
      expect(message.monoSessionCompletion?.status).toBe(status);
      expect(message.text).toContain("Partial work");
      expect(message.text).toContain("Agent stopped");
    },
  );

  it("deduplicates both pending and consumed receipts and preserves a paused queue", () => {
    const mono = newSession("codex", options.project);
    const message = monoSessionCompletionMessage(options);
    const next = enqueueMonoSessionCompletion(
      { ...mono, queueStatus: "paused" },
      message,
    );
    expect(next.queueStatus).toBe("paused");
    expect(enqueueMonoSessionCompletion(next, message)).toBe(next);
    const delivered = {
      ...mono,
      blocks: [
        {
          id: "notice",
          role: "user" as const,
          text: message.text,
          appRequestId: message.id,
          internal: true,
        },
      ],
    };
    expect(enqueueMonoSessionCompletion(delivered, message)).toBe(delivered);
  });

  it("dismisses a queued session report while keeping chat, other reports and the consumed transcript", () => {
    const target = monoSessionCompletionMessage(options);
    const other = monoSessionCompletionMessage({
      ...options,
      requestId: "other-request",
      sessionId: "other",
    });
    const chat = { id: "chat", text: "Explain the change", attachments: [] };
    const mono: Session = {
      ...newSession("codex", options.project),
      queueStatus: "paused",
      queuedMessages: [chat, target, other],
      blocks: [
        {
          id: "old-notice",
          role: "user",
          text: target.text,
          appRequestId: target.id,
          internal: true,
        },
      ],
    };
    const next = dismissQueuedMonoSessionCompletion(mono, "worker");
    expect(next.queuedMessages).toEqual([chat, other]);
    expect(next.queueStatus).toBe("paused");
    expect(next.blocks).toBe(mono.blocks);
    expect(dismissQueuedMonoSessionCompletion(next, "worker")).toBe(next);
  });

  it("removes reports from a saved batch and updates its identity, count and aggregate status", () => {
    // A save/load keeps the JSON text and metadata; it has no in-memory watches.
    const message = JSON.parse(
      JSON.stringify(groupedMessage()),
    ) as QueuedMessage;
    const mono: Session = {
      ...newSession("codex", options.project),
      queuedMessages: [message],
    };
    const next = dismissQueuedMonoSessionCompletion(mono, "failed");
    const remaining = next.queuedMessages![0];
    expect(remaining.id).toBe(message.id);
    expect(remaining.monoSessionCompletion).toEqual({
      sessionId: "stopped",
      title: "2 session results",
      status: "cancelled",
      sessionCount: 2,
    });
    expect(
      JSON.parse(remaining.text.split("\n\n")[1]).sessions.map(
        (entry: { sessionId: string }) => entry.sessionId,
      ),
    ).toEqual(["stopped", "completed"]);

    const last = dismissQueuedMonoSessionCompletion(next, "stopped");
    expect(last.queuedMessages![0].id).toBe(message.id);
    expect(last.queuedMessages![0].monoSessionCompletion).toEqual({
      sessionId: "completed",
      title: "Agent session",
      status: "completed",
    });
    expect(
      JSON.parse(last.queuedMessages![0].text.split("\n\n")[1]).sessionId,
    ).toBe("completed");
    expect(
      dismissQueuedMonoSessionCompletion(last, "completed").queuedMessages,
    ).toEqual([]);
  });

  it("leaves unrelated or unrecognized saved batches untouched", () => {
    const message = groupedMessage();
    const mono: Session = {
      ...newSession("codex", options.project),
      queuedMessages: [message],
    };
    expect(dismissQueuedMonoSessionCompletion(mono, "unrelated")).toBe(mono);
    const malformed: Session = {
      ...mono,
      queuedMessages: [{ ...message, text: "Incomplete saved report" }],
    };
    expect(dismissQueuedMonoSessionCompletion(malformed, "stopped")).toBe(
      malformed,
    );
  });

  it("bounds long results and points the Mono to the source conversation", () => {
    const message = monoSessionCompletionMessage({
      ...options,
      outcome: { status: "completed", text: "x".repeat(15_000) },
    });
    expect(message.text).toContain('"truncated":true');
    expect(message.text).toContain("app sessions.read");
    expect(message.text.length).toBeLessThan(13_000);
  });
});
