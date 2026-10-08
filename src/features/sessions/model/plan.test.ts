import { describe, expect, it } from "vitest";
import { newSession } from "./session";
import {
  buildPlanPrompt,
  canUseMonoDelegation,
  canUseMonoTurnIntent,
  hasActivePlanTurn,
  consumePlanCommand,
  isProviderFailureText,
  isReviewablePlan,
  monoTurnIntent,
  monoSubmissionIntent,
  planTurnPrompt,
} from "./plan";

describe("plan mode prompts", () => {
  it("keeps Mono turns read-only until Plan mode is explicitly turned off", () => {
    expect(monoTurnIntent(true)).toBe("plan");
    expect(monoTurnIntent(true, "plan")).toBe("plan");
    expect(canUseMonoTurnIntent(true, "build")).toBe(false);
    expect(canUseMonoTurnIntent(true, "orchestrate")).toBe(false);
    expect(canUseMonoDelegation(true)).toBe(false);
    expect(monoTurnIntent(false)).toBe("default");
    expect(canUseMonoTurnIntent(false, "build")).toBe(true);
    expect(canUseMonoDelegation(false)).toBe(true);
  });

  it("allows an approved Build through submission with build provider intent only", () => {
    const approved = {
      approvedPlanBuild: true,
      hasApprovedPlan: true,
      canStartBuild: true,
    };
    expect(monoSubmissionIntent(true, "build", approved)).toBe("build");
    expect(monoSubmissionIntent(true, "build")).toBeNull();
    expect(
      monoSubmissionIntent(true, "build", { ...approved, managed: true }),
    ).toBeNull();
    expect(
      monoSubmissionIntent(true, "build", { ...approved, appRequest: true }),
    ).toBeNull();
    expect(
      monoSubmissionIntent(true, "build", { ...approved, queued: true }),
    ).toBeNull();
    expect(monoSubmissionIntent(true, "orchestrate", approved)).toBeNull();
    expect(
      monoSubmissionIntent(true, "build", {
        ...approved,
        canStartBuild: false,
      }),
    ).toBeNull();
  });

  it("keeps a running plan turn read-only after the saved switch changes", () => {
    const session = newSession("codex", "/repo");
    session.busy = true;
    session.blocks = [
      { id: "plan-turn", role: "user", text: "Inspect this", intent: "plan" },
    ];
    expect(hasActivePlanTurn(session)).toBe(true);
    session.busy = false;
    session.backgroundTasks = ["shell"];
    expect(hasActivePlanTurn(session)).toBe(true);
    session.backgroundTasks = [];
    expect(hasActivePlanTurn(session)).toBe(false);
  });

  it("consumes only a leading /plan command", () => {
    expect(consumePlanCommand("/plan build a settings page")).toEqual({
      text: "build a settings page",
      planning: true,
    });
    expect(consumePlanCommand("  /PLAN\ninspect this")).toEqual({
      text: "inspect this",
      planning: true,
    });
    expect(consumePlanCommand("mention /plan in docs")).toEqual({
      text: "mention /plan in docs",
      planning: false,
    });
  });

  it("separates investigation from explicit approved-plan execution", () => {
    expect(planTurnPrompt("Add search")).toContain("do not modify files");
    expect(planTurnPrompt("Add search")).toContain("delegate implementation");
    const build = buildPlanPrompt("# Plan\n\n1. Add search");
    expect(build).toContain("explicitly approved");
    expect(build).toContain("# Plan\n\n1. Add search");
  });

  it("rejects provider blockers and ordinary commentary as fallback plans", () => {
    expect(isProviderFailureText("Upgrade your plan to continue")).toBe(true);
    expect(isReviewablePlan("Upgrade your plan to continue")).toBe(false);
    expect(
      isReviewablePlan("I checked the repository and found the issue."),
    ).toBe(false);
  });

  it("accepts structured markdown fallback plans", () => {
    expect(
      isReviewablePlan("# Plan\n\nInspect the flow and update the adapter."),
    ).toBe(true);
    expect(isReviewablePlan("1. Inspect the flow\n2. Update the adapter")).toBe(
      true,
    );
  });
});
