import { describe, expect, it } from "vitest";
import {
  completeOrchestrationProposal,
  orchestrationPlanningPrompt,
  proposalBlock,
  validateProposedTasks,
  type OrchestrationProposal,
} from "./orchestrationPlan";
import { newSession } from "./session";
import { sanitizeSessionForPersist } from "./sessionStore";
import { stopStreaming } from "./harness/apply";

const draft: OrchestrationProposal = {
  version: 1,
  leadId: "lead",
  cwd: "/repo",
  request: "Build settings",
  author: { harness: "claude", model: "claude:test", name: "Lead" },
  settings: {
    choices: [{ harness: "codex", model: "codex:test", name: "Worker" }],
    maxWorkers: 2,
  },
  status: "planning",
  title: "Planning",
  summary: "",
  tasks: [],
};
const task = {
  id: "ui",
  title: "Settings UI",
  prompt: "Build the view",
  harness: "codex",
  model: "codex:test",
  files: ["src/settings"],
  dependsOn: [],
};
const payload = {
  title: "Settings",
  summary: "Build the view, then validate",
  tasks: [task],
};

describe("orchestration proposals", () => {
  it("prompts the current lead with the available model catalog and no execution authority", () => {
    const prompt = orchestrationPlanningPrompt(draft.request, draft.settings);
    expect(prompt).toContain("do not edit files, start workers");
    expect(prompt).toContain('"model":"codex:test"');
    expect(prompt).toContain("until the user confirms");
    expect(prompt).toContain("<monocode_proposal>");
    expect(prompt).toContain("fewest useful tasks");
    expect(prompt).toContain("Do not ask the user to assemble a team");
    expect(prompt).toContain("disjoint files");
    expect(prompt).toContain("acceptance checks");
  });
  it("turns the lead's structured response into a ready card without changing the discovered catalog", () => {
    const result = completeOrchestrationProposal(
      draft,
      `Commentary\n<monocode_proposal>${JSON.stringify({ ...payload, settings: { choices: [] } })}</monocode_proposal>`,
    );
    expect(result.status).toBe("ready");
    expect(result.tasks).toEqual(payload.tasks);
    expect(result.settings).toEqual(draft.settings);
    expect(result.author).toEqual(draft.author);
  });
  it("accepts fenced JSON and rejects prose or a model outside the available catalog", () => {
    expect(
      completeOrchestrationProposal(
        draft,
        "```json\n" + JSON.stringify(payload) + "\n```",
      ).status,
    ).toBe("ready");
    expect(
      completeOrchestrationProposal(draft, "I will implement it now").status,
    ).toBe("invalid");
    const invalid = completeOrchestrationProposal(
      draft,
      JSON.stringify({
        ...payload,
        tasks: [{ ...task, model: "codex:unselected" }],
      }),
    );
    expect(invalid.status).toBe("invalid");
    expect(invalid.error).toContain("available catalog");
    expect(invalid.tasks).toEqual([]);
  });
  it("validates cycles, unknown dependencies and path escapes before execution", () => {
    const settings = draft.settings;
    expect(() =>
      validateProposedTasks([{ ...task, dependsOn: ["ui"] }], settings),
    ).toThrow("cycle");
    expect(() =>
      validateProposedTasks([{ ...task, dependsOn: ["missing"] }], settings),
    ).toThrow("unknown assignment");
    expect(() =>
      validateProposedTasks([{ ...task, files: ["../outside"] }], settings),
    ).toThrow("project-relative");
    expect(() => validateProposedTasks([task, task], settings)).toThrow(
      "unique",
    );
    expect(
      validateProposedTasks(
        [
          { ...task, dependsOn: ["data"] },
          { ...task, id: "data" },
        ],
        settings,
      ),
    ).toHaveLength(2);
  });
  it("keeps an explicitly selected worker effort and rejects malformed settings", () => {
    expect(
      validateProposedTasks(
        [
          {
            ...task,
            modelSettings: { reasoningEffort: "xhigh" },
          },
        ],
        draft.settings,
      )[0].modelSettings,
    ).toEqual({ reasoningEffort: "xhigh" });
    expect(() =>
      validateProposedTasks(
        [{ ...task, modelSettings: { reasoningEffort: 42 } }],
        draft.settings,
      ),
    ).toThrow("model settings");
  });
  it("keeps model edits and assignments through persistence", () => {
    const proposal = completeOrchestrationProposal(
      draft,
      JSON.stringify(payload),
    );
    proposal.tasks[0].prompt = "User edited the instructions";
    const session = {
      ...newSession("claude", "/repo"),
      blocks: [proposalBlock("proposal", proposal)],
    };
    expect(sanitizeSessionForPersist(session).blocks[0].orchestration).toEqual(
      proposal,
    );
  });
  it("makes interrupted planning non-executable on stop and reload", () => {
    const session = {
      ...newSession("claude", "/repo"),
      busy: true,
      blocks: [proposalBlock("proposal", draft)],
    };
    expect(stopStreaming(session).blocks[0].orchestration?.status).toBe(
      "invalid",
    );
    expect(
      sanitizeSessionForPersist(session).blocks[0].orchestration?.status,
    ).toBe("invalid");
  });
});
