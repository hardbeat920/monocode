import { describe, expect, it } from "vitest";
import {
  eventsFromAntigravityLine,
  modelsFromAntigravityOutput,
  nativeAntigravityModelId,
} from "./antigravityProtocol";

describe("antigravity catalog", () => {
  it("groups effort variants into one picker row", () => {
    const models = modelsFromAntigravityOutput(`
Fetching available models...
gemini-3.8-flash-high	Gemini 3.8 Flash (High)
gemini-3.8-flash-medium	Gemini 3.8 Flash (Medium)
gemini-3.8-flash-low	Gemini 3.8 Flash (Low)
claude-opus-4-6-thinking	Claude Opus 4.6 (Thinking)
gpt-oss-120b-medium	GPT-OSS 120B (Medium)
`);
    expect(models.map((model) => model.id)).toEqual([
      "antigravity:gemini-3.8-flash",
      "antigravity:claude-opus-4-6-thinking",
      "antigravity:gpt-oss-120b",
    ]);
    expect(models[0]?.name).toBe("Gemini 3.8 Flash");
    expect(models[0]?.nativeId).toBe("gemini-3.8-flash-high");
    expect(models[0]?.settings?.[0]?.id).toBe("effort");
    expect(models[0]?.settings?.[0]?.options.map((option) => option.value)).toEqual(
      ["high", "medium", "low"],
    );
  });

  it("applies the selected effort to the native model id", () => {
    expect(
      nativeAntigravityModelId("gemini-3.8-flash-high", { effort: "low" }),
    ).toBe("gemini-3.8-flash-low");
  });
});

describe("antigravity stream events", () => {
  it("binds the conversation and streams text", () => {
    expect(
      eventsFromAntigravityLine(
        JSON.stringify({
          event: "init",
          conversation_id: "conv-1",
          init: { model: "gemini-3.8-flash-low" },
        }),
      ),
    ).toEqual([
      { type: "session.providerBound", providerSessionId: "conv-1" },
    ]);
    expect(
      eventsFromAntigravityLine(
        JSON.stringify({
          event: "step_update",
          step_update: { step_type: "agent_response", text_delta: "Hi\n" },
        }),
      ),
    ).toEqual([{ type: "message.delta", text: "Hi\n" }]);
    expect(
      eventsFromAntigravityLine(
        JSON.stringify({
          event: "result",
          result: { status: "SUCCESS", response: "Hi\n" },
        }),
      ),
    ).toEqual([{ type: "message.completed" }]);
  });
});
