import { describe, expect, it } from "vitest";
import {
  hermesCurrentModelId,
  hermesBackgroundDispatch,
  hermesModeId,
  hermesPromptBlocks,
  hermesSessionId,
  hermesStderrAuthError,
  hermesStartupError,
  modelsFromHermesSession,
} from "./hermesProtocol";

describe("Hermes ACP protocol", () => {
  it("maps access modes onto Hermes edit approval modes", () => {
    expect(hermesModeId("supervised")).toBe("default");
    expect(hermesModeId("auto-accept-edits")).toBe("accept_edits");
    expect(hermesModeId("auto")).toBe("dont_ask");
    expect(hermesModeId("full-access")).toBe("dont_ask");
    expect(hermesModeId("full-access", true)).toBe("default");
  });

  it("uses standard ACP text and image prompt blocks", () => {
    expect(
      hermesPromptBlocks("  inspect this  ", [
        {
          id: "image-1",
          name: "screen.png",
          mimeType: "image/png",
          kind: "image",
          size: 4,
          data: "AAAA",
        },
      ]),
    ).toEqual([
      { type: "text", text: "inspect this" },
      { type: "image", mimeType: "image/png", data: "AAAA" },
    ]);
  });

  it("recovers Hermes background delegation handles from ACP tool content", () => {
    expect(
      hermesBackgroundDispatch({
        sessionId: "hermes-session-1",
        update: {
          sessionUpdate: "tool_call_update",
          toolCallId: "tool-delegate",
          status: "completed",
          content: [
            {
              type: "content",
              content: {
                type: "text",
                text: JSON.stringify({
                  status: "dispatched",
                  mode: "background",
                  delegation_id: "deleg_1234",
                  live_transcripts: [
                    "/tmp/deleg_1234/task-0.log",
                    "/tmp/deleg_1234/task-1.log",
                  ],
                }),
              },
            },
          ],
        },
      }),
    ).toEqual({
      callId: "tool-delegate",
      delegationId: "deleg_1234",
      transcripts: ["/tmp/deleg_1234/task-0.log", "/tmp/deleg_1234/task-1.log"],
    });
  });

  it("reads Hermes model state and puts its current model first", () => {
    const setup = {
      sessionId: "hermes-session-1",
      models: {
        currentModelId: "nous:hermes-4",
        availableModels: [
          { modelId: "openrouter:gpt-5", name: "OpenRouter · GPT-5" },
          { modelId: "nous:hermes-4", name: "Nous · Hermes 4" },
          { modelId: "nous:hermes-4", name: "duplicate" },
        ],
      },
    };

    expect(hermesSessionId(setup)).toBe("hermes-session-1");
    expect(hermesCurrentModelId(setup)).toBe("nous:hermes-4");
    expect(modelsFromHermesSession(setup)).toEqual([
      {
        id: "hermes:nous:hermes-4",
        harness: "hermes",
        name: "Nous · Hermes 4",
        nativeId: "nous:hermes-4",
      },
      {
        id: "hermes:openrouter:gpt-5",
        harness: "hermes",
        name: "OpenRouter · GPT-5",
        nativeId: "openrouter:gpt-5",
      },
    ]);
  });

  it("accepts snake-case ACP response fields", () => {
    const setup = {
      session_id: "hermes-session-2",
      models: {
        current_model_id: "local:model",
        available_models: [{ model_id: "local:model", name: "Local" }],
      },
    };
    expect(hermesSessionId(setup)).toBe("hermes-session-2");
    expect(modelsFromHermesSession(setup)[0]?.nativeId).toBe("local:model");
  });

  it("adds actionable setup help to credential errors", () => {
    const error = hermesStartupError(new Error("provider is not configured"));
    expect(error.message).toContain("provider is not configured");
    expect(error.message).toContain("hermes model");
    expect(error.message).toContain("hermes acp --check");
  });

  it("does not turn Hermes provider-health warnings into chat errors", () => {
    expect(
      hermesStderrAuthError(
        "2026-09-17 10:24:42 [WARNING] agent.credential_pool: Copilot token exchange degraded to RAW token (exchange unavailable); enterprise-only models may 400 with model_not_available_for_integrator until exchange recovers.",
      ),
    ).toBeNull();
  });

  it("surfaces explicit Hermes authentication failures with setup help", () => {
    const message = hermesStderrAuthError(
      "2026-09-17 10:24:42 [ERROR] agent.provider: No LLM provider configured",
    );
    expect(message).toContain("No LLM provider configured");
    expect(message).toContain("hermes model");
  });
});
