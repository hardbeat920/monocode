import { expect, it } from "vitest";
import { newSession, type Block } from "../../features/sessions/model/session";
import {
  proposalBlock,
  type OrchestrationProposal,
} from "../../features/orchestration/model/orchestrationPlan";
import { applyHarnessEvent } from "../../integrations/harness/core/apply";
import { finalizeSubmittedTurn } from "./finalizeSubmittedTurn";

const options = {
  intent: "default" as const,
  planEventKey: "submitted-plan",
  nativePlanSeen: false,
  providerFailureSeen: false,
  buildSucceeded: true,
  proposalText: "",
};
const draft: OrchestrationProposal = {
  version: 1,
  leadId: "lead",
  cwd: "/repo",
  request: "Fix settings",
  author: { harness: "pi", model: "pi:test", name: "Lead" },
  settings: {
    choices: [{ harness: "codex", model: "codex:test", name: "Worker" }],
    maxWorkers: 2,
  },
  status: "planning",
  title: "Planning",
  summary: "",
  tasks: [],
};
const response = JSON.stringify({
  title: "Settings",
  summary: "Fix the settings view",
  tasks: [
    {
      id: "ui",
      title: "Settings",
      prompt: "Fix settings",
      harness: "codex",
      model: "codex:test",
      files: ["src/settings"],
      dependsOn: [],
    },
  ],
});

it("finishes the submitted proposal while preserving a newer run and its approval", () => {
  const submitted = {
    ...newSession("pi", "/repo"),
    blocks: [proposalBlock("proposal", draft)],
  };
  const newer: Block = {
    id: "new-answer",
    role: "assistant",
    text: "Working",
    streaming: true,
  };
  const resumed = {
    ...applyHarnessEvent(submitted, { type: "turn.activity", active: true }),
    blocks: [...submitted.blocks, newer],
    backgroundTasks: ["pi-subagents"],
    pendingQuestion: { requestId: 1, questions: [] },
  };
  const finalized = finalizeSubmittedTurn(resumed, submitted, {
    ...options,
    intent: "orchestrate",
    proposalId: "proposal",
    proposalDraft: draft,
    proposalText: response,
  });
  expect(finalized.blocks[0].orchestration?.status).toBe("ready");
  expect(finalized.blocks[0].streaming).toBe(false);
  expect(finalized.blocks[1]).toBe(newer);
  expect(finalized.busy).toBe(true);
  expect(finalized.providerActive).toBe(true);
  expect(finalized.backgroundTasks).toBe(resumed.backgroundTasks);
  expect(finalized.pendingQuestion).toBe(resumed.pendingQuestion);
});

it("promotes the old answer, even when a newer run has a user message and an error-like answer", () => {
  const submitted = {
    ...newSession("pi", "/repo"),
    blocks: [
      { id: "old-user", role: "user", text: "Plan this" },
      {
        id: "old-answer",
        role: "assistant",
        text: "# Plan\n- Fix it\n- Test it",
      },
    ] as Block[],
  };
  const newer: Block[] = [
    { id: "new-user", role: "user", text: "Another request" },
    {
      id: "new-answer",
      role: "assistant",
      text: "Upgrade your plan to continue",
      streaming: true,
    },
  ];
  const resumed = {
    ...submitted,
    providerActive: true,
    busy: true,
    blocks: [...submitted.blocks, ...newer],
  };
  const finalized = finalizeSubmittedTurn(resumed, submitted, {
    ...options,
    intent: "plan",
  });
  expect(finalized.blocks[1].role).toBe("plan");
  expect(finalized.blocks[1].plan?.key).toBe("submitted-plan");
  expect(finalized.blocks.slice(2)).toEqual(newer);
  expect(finalized.busy).toBe(true);
});

it.each([true, false])(
  "updates the approved plan without stopping a new run, success=%s",
  (buildSucceeded) => {
    const submitted = {
      ...newSession("pi", "/repo"),
      blocks: [
        {
          id: "approved",
          role: "plan",
          text: "# Plan",
          plan: { status: "building" },
        },
        { id: "user", role: "user", text: "Build it" },
      ] as Block[],
    };
    const resumed = { ...submitted, providerActive: true, busy: true };
    const finalized = finalizeSubmittedTurn(resumed, submitted, {
      ...options,
      intent: "build",
      approvedPlanId: "approved",
      buildSucceeded,
    });
    expect(finalized.blocks[0].plan?.status).toBe(
      buildSucceeded ? "built" : "ready",
    );
    expect(finalized.providerActive).toBe(true);
  },
);

it("keeps edits to unrelated plans that arrived during the checkpoint", () => {
  const submitted = {
    ...newSession("pi", "/repo"),
    blocks: [
      {
        id: "old-approval",
        role: "approval",
        text: "Allow?",
        approval: { requestId: 1 },
      },
      {
        id: "older-plan",
        role: "plan",
        text: "Old text",
        plan: { status: "ready" },
      },
      { id: "user", role: "user", text: "Continue" },
    ] as Block[],
  };
  const edited = { ...submitted.blocks[1], text: "Edited during checkpoint" };
  const resumed = {
    ...submitted,
    providerActive: true,
    busy: true,
    blocks: [edited, submitted.blocks[2]],
  };
  expect(finalizeSubmittedTurn(resumed, submitted, options).blocks[0]).toBe(
    edited,
  );
});

it("settles idle runs and still cleans up provider failures", () => {
  const submitted = {
    ...newSession("pi", "/repo"),
    busy: true,
    backgroundTasks: ["work"],
  };
  expect(finalizeSubmittedTurn(submitted, submitted, options).busy).toBe(false);
  const active = { ...submitted, providerActive: true };
  const failed = finalizeSubmittedTurn(active, submitted, {
    ...options,
    providerFailureSeen: true,
    buildSucceeded: false,
  });
  expect(failed.busy).toBe(false);
  expect(failed.providerActive).toBeUndefined();
  expect(failed.backgroundTasks).toBeUndefined();
});
