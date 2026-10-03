import { describe, expect, it } from "vitest";
import {
  acceptProviderDelivery,
  beginProviderDelivery,
  canApplyRunningConfiguration,
  canResumeProviderBinding,
  failProviderDelivery,
  failUnstartedProviderRequest,
  markProviderContextDelivered,
  providerBinding,
  recordProviderBound,
  recordProviderContextUsage,
  rememberProviderBinding,
  requiresFreshProviderBinding,
  runningProviderSelection,
  sanitizeProviderContext,
  settleProviderBinding,
  type ProviderBinding,
} from "./providerContext";
import { buildPortableContext } from "./portableContext";
import { newSession, type Session } from "./session";

const cwd = "/tmp/project";
const source: ProviderBinding = {
  harness: "claude",
  cwd,
  providerSessionId: "claude-native",
  deliveredThroughBlockId: "a1",
};

function switched(): Session {
  const initial = {
    ...newSession("claude", cwd),
    providerSessionId: source.providerSessionId,
    blocks: [
      {
        id: "u1",
        role: "user" as const,
        text: "Keep the early instruction exactly.",
      },
      { id: "a1", role: "assistant" as const, text: "Source response." },
    ],
  };
  return {
    ...rememberProviderBinding(initial, source),
    harness: "codex",
    providerSessionId: undefined,
    pendingSwitch: {
      from: "claude",
      fromModel: "claude:sonnet",
      fromSettings: {},
      fromProviderSessionId: source.providerSessionId,
    },
  };
}

function preparing(): Session {
  return beginProviderDelivery(switched(), {
    switchId: "switch-1",
    from: "claude",
    to: "codex",
    cwd,
    currentUserBlockId: "u2",
    sourceThroughBlockId: "a1",
    includedBlockIds: ["u1", "a1"],
    omittedBlockIds: [],
  });
}

describe("durable provider bindings", () => {
  it("routes active approvals and questions to the running source after picker retargeting", () => {
    const session = { ...switched(), busy: true };
    const sourceSelection = {
      harness: "claude" as const,
      model: "claude:sonnet",
      modelSettings: { effort: "high" },
    };
    expect(runningProviderSelection(session, sourceSelection)).toEqual(
      sourceSelection,
    );
    const preparedTarget = {
      harness: "codex" as const,
      model: "codex:gpt",
      modelSettings: {},
    };
    expect(runningProviderSelection(session, preparedTarget)).toEqual(
      preparedTarget,
    );
    expect(
      runningProviderSelection({ ...session, busy: false }, sourceSelection)
        .harness,
    ).toBe("codex");
  });

  it("retains the running model while a same-provider picker selection changes", () => {
    const session = {
      ...newSession("claude", cwd),
      busy: true,
      model: "claude:new",
    };
    const running = {
      harness: "claude" as const,
      model: "claude:original",
      modelSettings: {},
    };
    expect(runningProviderSelection(session, running).model).toBe(
      "claude:original",
    );
    expect(
      runningProviderSelection({ ...session, busy: false }, running).model,
    ).toBe("claude:new");
  });

  it("rejects late config and window events after a same-provider picker change", () => {
    const running = {
      harness: "claude" as const,
      model: "claude:original",
      modelSettings: { effort: "high" },
    };
    const session = { ...newSession("claude", cwd), ...running, busy: true };
    expect(canApplyRunningConfiguration(session, running)).toBe(true);
    expect(
      canApplyRunningConfiguration(
        { ...session, model: "claude:next" },
        running,
      ),
    ).toBe(false);
    expect(
      canApplyRunningConfiguration(
        { ...session, modelSettings: { effort: "low" } },
        running,
      ),
    ).toBe(false);
    expect(
      canApplyRunningConfiguration(
        { ...session, model: "claude:normalized" },
        running,
        { running: 2, selected: 2 },
      ),
    ).toBe(true);
    expect(
      canApplyRunningConfiguration(
        { ...session, model: "claude:next" },
        running,
        { running: 2, selected: 3 },
      ),
    ).toBe(false);
  });
  it("seeds a legacy native binding without changing its logical session", () => {
    const session = {
      ...newSession("claude", cwd),
      providerSessionId: "legacy",
      blocks: [{ id: "u", role: "user" as const, text: "Hello" }],
    };
    expect(providerBinding(session, "claude", cwd)).toEqual({
      harness: "claude",
      cwd,
      providerSessionId: "legacy",
      providerAccountId: undefined,
      deliveredThroughBlockId: "u",
    });
    expect(session.providerContext).toBeUndefined();
  });

  it("requires matching provider account and working directory", () => {
    const session = rememberProviderBinding(switched(), {
      ...source,
      providerAccountId: "work",
    });
    expect(
      providerBinding(session, "claude", cwd, "work")?.providerSessionId,
    ).toBe("claude-native");
    expect(providerBinding(session, "claude", cwd, "personal")).toBeUndefined();
    expect(
      providerBinding(session, "claude", "/tmp/other", "work"),
    ).toBeUndefined();
  });

  it("requires fresh history when an edited resend removed the saved native boundary", () => {
    const session = switched();
    const saved = providerBinding(session, "claude", cwd);
    expect(canResumeProviderBinding(session, saved)).toBe(true);
    const recovered = {
      ...session,
      blocks: [
        session.blocks[0],
        {
          id: "b1",
          role: "assistant" as const,
          text: "The later provider response.",
        },
      ],
    };
    const stale = providerBinding(recovered, "claude", cwd);
    expect(stale?.deliveredThroughBlockId).toBe("a1");
    expect(canResumeProviderBinding(recovered, stale)).toBe(false);
    const target = canResumeProviderBinding(recovered, stale)
      ? stale
      : undefined;
    expect(
      buildPortableContext(recovered, {
        afterBlockId: target?.deliveredThroughBlockId,
      }).items.map((item) => item.sourceBlockId),
    ).toEqual(["u1", "b1"]);
    expect(
      canResumeProviderBinding(session, {
        ...source,
        deliveredThroughBlockId: undefined,
      }),
    ).toBe(false);
    expect(canResumeProviderBinding(session, undefined)).toBe(false);
  });

  it("restores legacy native identities through the default account picker", () => {
    const legacy = {
      ...newSession("claude", cwd),
      providerSessionId: "legacy-native",
    };
    expect(
      providerBinding(legacy, "claude", cwd, "default")?.providerSessionId,
    ).toBe("legacy-native");
    const pending = { ...switched(), providerContext: undefined };
    expect(
      providerBinding(pending, "claude", cwd, "default")?.providerSessionId,
    ).toBe("claude-native");
    expect(
      providerBinding(switched(), "claude", cwd, "default")?.providerSessionId,
    ).toBe("claude-native");
  });

  it("replaces default account aliases without merging named accounts", () => {
    let session = rememberProviderBinding(switched(), {
      ...source,
      providerAccountId: "work",
      providerSessionId: "work-native",
    });
    session = rememberProviderBinding(session, {
      ...source,
      providerAccountId: "default",
      providerSessionId: "default-native",
    });
    expect(session.providerContext?.bindings).toHaveLength(2);
    expect(providerBinding(session, "claude", cwd)?.providerSessionId).toBe(
      "default-native",
    );
    expect(providerBinding(session, "claude", cwd, "")?.providerSessionId).toBe(
      "default-native",
    );
    expect(
      providerBinding(session, "claude", cwd, "work")?.providerSessionId,
    ).toBe("work-native");
    expect(providerBinding(session, "claude", cwd, "personal")).toBeUndefined();
  });

  it("keeps the target selection when a still-running source reports its identity", () => {
    const session = recordProviderBound(
      switched(),
      "claude",
      cwd,
      "new-source-native",
    );
    expect(session.harness).toBe("codex");
    expect(session.providerSessionId).toBeUndefined();
    expect(session.pendingSwitch?.fromProviderSessionId).toBe(
      "new-source-native",
    );
    expect(providerBinding(session, "claude", cwd)?.providerSessionId).toBe(
      "new-source-native",
    );
  });

  it("does not treat provider startup as turn acceptance", () => {
    const session = recordProviderBound(
      preparing(),
      "codex",
      cwd,
      "target-native",
    );
    expect(session.providerContext?.delivery?.status).toBe("preparing");
    expect(session.pendingSwitch?.fromProviderSessionId).toBe("claude-native");
    expect(session.providerContext?.delivery?.targetProviderSessionId).toBe(
      "target-native",
    );
  });

  it("requires a fresh target after restart before or after import", () => {
    for (const session of [
      preparing(),
      markProviderContextDelivered(
        preparing(),
        "switch-1",
        "native",
        "target-native",
      ),
    ]) {
      const restored = {
        ...session,
        providerContext: sanitizeProviderContext(
          JSON.parse(JSON.stringify(session.providerContext)),
        ),
      };
      expect(requiresFreshProviderBinding(restored, "codex", cwd)).toBe(true);
      expect(providerBinding(restored, "claude", cwd)?.providerSessionId).toBe(
        "claude-native",
      );
    }
  });

  it("keeps native history import distinct from an accepted current request", () => {
    const imported = markProviderContextDelivered(
      preparing(),
      "switch-1",
      "native",
      "target-native",
    );
    expect(imported.providerContext?.delivery?.status).toBe("imported");
    expect(imported.pendingSwitch).toBeDefined();
    const accepted = acceptProviderDelivery(imported, "switch-1");
    expect(accepted.providerContext?.delivery?.status).toBe("accepted");
    expect(accepted.pendingSwitch).toBeUndefined();
    expect(requiresFreshProviderBinding(accepted, "codex", cwd)).toBe(false);
  });

  it("discards an uncertain target while preserving a retryable source", () => {
    const bound = recordProviderBound(
      markProviderContextDelivered(preparing(), "switch-1", "native"),
      "codex",
      cwd,
      "uncertain-target",
    );
    const failed = failProviderDelivery(bound, "switch-1");
    expect(failed.providerContext?.delivery?.status).toBe("uncertain");
    expect(failed.providerSessionId).toBeUndefined();
    expect(providerBinding(failed, "codex", cwd)).toBeUndefined();
    expect(providerBinding(failed, "claude", cwd)?.providerSessionId).toBe(
      "claude-native",
    );
    expect(failed.pendingSwitch).toEqual(bound.pendingSwitch);
  });

  it("keeps an unaccepted current request as one draft and excludes it from retry context", () => {
    const session = {
      ...preparing(),
      blocks: [
        ...preparing().blocks,
        {
          id: "u2",
          role: "user" as const,
          text: "Retry this exact request once",
        },
      ],
    };
    const failed = failProviderDelivery(
      markProviderContextDelivered(
        session,
        "switch-1",
        "native",
        "target-native",
      ),
      "switch-1",
    );
    expect(failed.blocks.filter((block) => block.id === "u2")).toEqual([
      {
        id: "u2",
        role: "user",
        text: "Retry this exact request once",
        draft: true,
      },
    ]);
    expect(
      buildPortableContext(failed).items.map((item) => item.text),
    ).not.toContain("Retry this exact request once");
    expect(failed.pendingSwitch?.fromProviderSessionId).toBe("claude-native");
  });

  it("preserves the unsent request when cancellation happens during asset snapshot preparation", () => {
    const session: Session = {
      ...switched(),
      blocks: [
        ...switched().blocks,
        {
          id: "preflight",
          role: "handoff",
          text: "",
          handoff: { from: "claude", to: "codex", status: "preparing" },
        },
        { id: "unsent", role: "user", text: "Send this request once" },
      ],
    };
    const cancelled = failUnstartedProviderRequest(session);
    expect(cancelled.blocks.filter((block) => block.id === "unsent")).toEqual([
      {
        id: "unsent",
        role: "user",
        text: "Send this request once",
        draft: true,
      },
    ]);
    expect(
      buildPortableContext(cancelled).items.map((item) => item.text),
    ).not.toContain("Send this request once");
    expect(cancelled.pendingSwitch).toEqual(session.pendingSwitch);
  });

  it("does not mark an active source request as draft when only the picker changed", () => {
    const session = { ...switched(), busy: true };
    expect(failUnstartedProviderRequest(session)).toBe(session);
  });

  it("records the full coverage receipt when stale native resume required fallback", () => {
    const session = markProviderContextDelivered(
      preparing(),
      "switch-1",
      "native",
      "fresh-native",
      {
        includedBlockIds: ["older-user", "u1", "a1"],
        sourceThroughBlockId: "a1",
      },
    );
    expect(session.providerContext?.delivery?.includedBlockIds).toEqual([
      "older-user",
      "u1",
      "a1",
    ]);
    expect(session.providerContext?.delivery?.targetProviderSessionId).toBe(
      "fresh-native",
    );
  });

  it("ignores a late receipt for another switch", () => {
    const session = preparing();
    expect(markProviderContextDelivered(session, "stale", "native")).toBe(
      session,
    );
    expect(acceptProviderDelivery(session, "stale")).toBe(session);
    expect(failProviderDelivery(session, "stale")).toBe(session);
  });

  it("preserves accepted target state after a later generation fails", () => {
    const accepted = acceptProviderDelivery(preparing(), "switch-1");
    expect(failProviderDelivery(accepted, "switch-1")).toBe(accepted);
  });

  it("ignores late delivery and acceptance receipts after cancellation", () => {
    const cancelled = failProviderDelivery(preparing(), "switch-1");
    expect(
      markProviderContextDelivered(
        cancelled,
        "switch-1",
        "native",
        "late-native",
      ),
    ).toBe(cancelled);
    expect(acceptProviderDelivery(cancelled, "switch-1")).toBe(cancelled);
  });

  it("does not clear a later picker intent when an accepted turn repeats its receipt", () => {
    const accepted = acceptProviderDelivery(preparing(), "switch-1");
    const next = {
      ...accepted,
      pendingSwitch: {
        from: "codex" as const,
        fromModel: "codex:gpt",
        fromSettings: {},
        fromProviderSessionId: "codex-native",
      },
    };
    expect(acceptProviderDelivery(next, "switch-1")).toBe(next);
  });

  it("retains source usage separately from the selected target meter", () => {
    const sourceSession = {
      ...newSession("claude", cwd),
      providerSessionId: "native",
      context: { used: 18_000, window: 100_000 },
    };
    const binding = providerBinding(sourceSession, "claude", cwd)!;
    expect(binding.contextUsed).toBe(18_000);
    expect(binding.contextWindow).toBe(100_000);
    const saved = sanitizeProviderContext({
      version: 1,
      bindings: [binding, { ...source, contextUsed: NaN, contextWindow: -1 }],
    });
    expect(saved?.bindings[0].contextUsed).toBeUndefined();
    expect(saved?.bindings[0].contextWindow).toBeUndefined();
    const current = {
      ...switched(),
      context: { used: 3_000, window: 200_000 },
    };
    const updated = recordProviderContextUsage(current, "claude", cwd, {
      used: 20_000,
      window: 100_000,
    });
    expect(providerBinding(updated, "claude", cwd)?.contextUsed).toBe(20_000);
    expect(updated.context).toEqual(current.context);
  });

  it("resumes A with only the complete B interval on switchback", () => {
    let session = recordProviderBound(
      preparing(),
      "codex",
      cwd,
      "codex-native",
    );
    session = acceptProviderDelivery(session, "switch-1");
    session = {
      ...session,
      blocks: [
        ...session.blocks,
        { id: "u2", role: "user", text: "B request" },
        { id: "b1", role: "assistant", text: "B response" },
      ],
    };
    session = settleProviderBinding(session, "codex", cwd);
    const a = providerBinding(session, "claude", cwd)!;
    const b = providerBinding(session, "codex", cwd)!;
    const context = buildPortableContext(session, {
      afterBlockId: a.deliveredThroughBlockId,
    });
    expect(a.providerSessionId).toBe("claude-native");
    expect(b.deliveredThroughBlockId).toBe("b1");
    expect(context.items.map((item) => item.text)).toEqual([
      "B request",
      "B response",
    ]);
    expect(session.id).toBeDefined();
    expect(session.cwd).toBe(cwd);
  });

  it("updates a source boundary when it settles after the picker changed", () => {
    const session = {
      ...switched(),
      blocks: [
        ...switched().blocks,
        { id: "a2", role: "assistant" as const, text: "Completed source turn" },
      ],
    };
    const settled = settleProviderBinding(session, "claude", cwd);
    expect(
      providerBinding(settled, "claude", cwd)?.deliveredThroughBlockId,
    ).toBe("a2");
    expect(settled.harness).toBe("codex");
    expect(settled.providerSessionId).toBeUndefined();
  });

  it("rejects malformed records and deduplicates saved bindings", () => {
    expect(
      sanitizeProviderContext({ version: 2, bindings: [] }),
    ).toBeUndefined();
    const state = sanitizeProviderContext({
      version: 1,
      bindings: [
        null,
        { harness: "unknown", cwd, providerSessionId: "bad" },
        source,
        { ...source, providerSessionId: "latest" },
      ],
      delivery: { status: "accepted" },
    });
    expect(state?.bindings).toEqual([
      { ...source, providerSessionId: "latest" },
    ]);
    expect(state?.delivery).toBeUndefined();
  });
});
