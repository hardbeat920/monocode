import { describe, expect, it } from "vitest";
import type { AgentModel } from "./models";
import { isOpus55Model, opusStage } from "./opusWelcome";

const opus: AgentModel = {
  id: "claude:opus-5-5",
  harness: "claude",
  name: "Claude Opus 5.5",
  nativeId: "claude-opus-5-5",
};

describe("Opus 5.5 welcome", () => {
  it("recognizes Opus 5.5 in catalog names and IDs without matching its siblings", () => {
    expect(isOpus55Model(opus)).toBe(true);
    expect(
      isOpus55Model({
        ...opus,
        id: "pi:new",
        name: "New",
        nativeId: "anthropic/claude-opus-5-5-20260901",
      }),
    ).toBe(true);
    expect(
      isOpus55Model({
        ...opus,
        id: "opencode:x",
        name: "Opus 5.5",
        nativeId: undefined,
      }),
    ).toBe(true);
    expect(
      isOpus55Model({
        ...opus,
        id: "claude:opus-5",
        name: "Claude Opus 5",
        nativeId: "claude-opus-5",
      }),
    ).toBe(false);
    expect(
      isOpus55Model({
        ...opus,
        id: "claude:opus-5-6",
        name: "Opus 5.6",
        nativeId: "claude-opus-5-6",
      }),
    ).toBe(false);
    expect(
      isOpus55Model({
        ...opus,
        id: "claude:opus-5-55",
        name: "Opus 5.55",
        nativeId: "claude-opus-5-55",
      }),
    ).toBe(false);
  });

  it("stages the scene under a centered composer, or above a docked one", () => {
    expect(opusStage(800, { top: 360, bottom: 480 })).toEqual({
      top: 480,
      height: 320,
    });
    expect(opusStage(800, { top: 680, bottom: 780 })).toEqual({
      top: 440,
      height: 240,
    });
    expect(opusStage(200, { top: 100, bottom: 190 })).toEqual({
      top: 0,
      height: 100,
    });
    expect(opusStage(800)).toEqual({ top: 440, height: 360 });
  });
});
