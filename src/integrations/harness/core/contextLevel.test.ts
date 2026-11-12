import { describe, expect, it } from "vitest";
import { aggregateModelCalls, withLiveContext } from "./contextLevel";

describe("aggregateModelCalls", () => {
  it("treats one call as the window level", () => {
    expect(aggregateModelCalls({ modelCalls: 1, totalTokens: 100 })).toBe(false);
    expect(aggregateModelCalls({ totalTokens: 100 })).toBe(false);
  });

  it("treats several calls as turn spend", () => {
    expect(aggregateModelCalls({ modelCalls: 4 })).toBe(true);
    expect(aggregateModelCalls({ numTurns: 18 })).toBe(true);
  });
});

describe("withLiveContext", () => {
  it("reads the envelope level when the update has none", () => {
    expect(
      withLiveContext({ _meta: { totalTokens: 68_587 } }, [
        { type: "message.delta", text: "hi" },
      ]),
    ).toEqual([
      { type: "message.delta", text: "hi" },
      { type: "context", used: 68_587 },
    ]);
  });

  it("leaves an explicit usage level alone", () => {
    const events = [{ type: "context" as const, used: 32_768, window: 131_072 }];
    expect(withLiveContext({ _meta: { totalTokens: 99 } }, events)).toBe(events);
  });

  it("ignores a missing or empty level", () => {
    const events = [{ type: "message.delta" as const, text: "hi" }];
    expect(withLiveContext(null, events)).toBe(events);
    expect(withLiveContext({ _meta: { totalTokens: 0 } }, events)).toBe(events);
  });
});
