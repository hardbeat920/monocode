import { describe, expect, it } from "vitest";
import {
  buildPlanPrompt,
  consumePlanCommand,
  isProviderFailureText,
  isReviewablePlan,
  planTurnPrompt,
} from "./plan";

describe("plan mode prompts", () => {
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
    const build = buildPlanPrompt("# Plan\n\n1. Add search");
    expect(build).toContain("explicitly approved");
    expect(build).toContain("# Plan\n\n1. Add search");
  });

  it("rejects provider blockers and ordinary commentary as fallback plans", () => {
    expect(isProviderFailureText("Upgrade your plan to continue")).toBe(true);
    expect(isReviewablePlan("Upgrade your plan to continue")).toBe(false);
    expect(isReviewablePlan("I checked the repository and found the issue.")).toBe(
      false,
    );
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
