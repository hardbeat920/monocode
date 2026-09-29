import { describe, expect, it } from "vitest";
import { makeConstellation } from "./OrchestratorConstellation";
import { planStepCount } from "./PlanStepsBurst";
import { shouldCelebrateTurn } from "./turnCelebration";

describe("turn celebrations", () => {
  it("celebrates only a turn sent moments ago", () => {
    const now = 1_000_000;
    expect(shouldCelebrateTurn("fresh", now - 500, now)).toBe(true);
    expect(shouldCelebrateTurn("old", now - 60_000, now)).toBe(false);
    expect(shouldCelebrateTurn("unknown", undefined, now)).toBe(false);
  });

  it("fits a short plan into a narrow bubble and caps a wide one", () => {
    expect(planStepCount(40)).toBe(2);
    expect(planStepCount(260)).toBe(4);
    expect(planStepCount(900)).toBe(6);
  });

  it("keeps every orchestrator node inside the bubble", () => {
    for (const [width, height] of [
      [60, 36],
      [320, 36],
      [720, 140],
    ]) {
      for (const random of [() => 0, () => 0.999, Math.random]) {
        const { hub, nodes } = makeConstellation(width, height, random);
        expect(hub.y).toBeLessThanOrEqual(height);
        for (const node of nodes) {
          expect(node.x).toBeGreaterThanOrEqual(0);
          expect(node.x).toBeLessThanOrEqual(width);
          expect(node.y).toBeGreaterThanOrEqual(0);
          expect(node.y).toBeLessThanOrEqual(height);
        }
      }
    }
  });

  it("fans agents out from the lead before any of them hand off", () => {
    const { hub, nodes } = makeConstellation(480, 80, () => 0.5);
    const agents = nodes.filter((node) => !node.child);
    const children = nodes.filter((node) => node.child);
    expect(agents.every((node) => node.from === hub)).toBe(true);
    expect(children.length).toBeGreaterThan(0);
    for (const child of children) {
      const parent = agents.find(
        (agent) => agent.x === child.from.x && agent.y === child.from.y,
      );
      expect(parent).toBeDefined();
      expect(child.delay).toBeGreaterThanOrEqual(parent!.delay + parent!.draw);
    }
  });
});
