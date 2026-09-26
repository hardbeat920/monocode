import { describe, expect, it } from "vitest";
import {
  COPILOT_AUTH_HELP,
  copilotCurrentModelId,
  copilotMetricsFromPromptResult,
  copilotModeId,
  copilotSessionId,
  copilotStderrAuthError,
  modelsFromCopilotSession,
} from "./copilotProtocol";

describe("Copilot ACP protocol", () => {
  it("keeps approvals in agent mode except planning and full access", () => {
    const base = "https://agentclientprotocol.com/protocol/session-modes#";
    expect(copilotModeId("supervised")).toBe(`${base}agent`);
    expect(copilotModeId("auto-accept-edits")).toBe(`${base}agent`);
    expect(copilotModeId("auto")).toBe(`${base}agent`);
    expect(copilotModeId("full-access")).toBe(`${base}autopilot`);
    expect(copilotModeId("full-access", true)).toBe(`${base}plan`);
  });

  it("maps prompt usage including Copilot cache fields without leaking unsupported totals", () => {
    expect(copilotMetricsFromPromptResult({
      stopReason: "end_turn",
      usage: {
        inputTokens: 100,
        outputTokens: 12,
        totalTokens: 112,
        thoughtTokens: 7,
        cachedReadTokens: 30,
        cachedWriteTokens: 20,
      },
    })).toEqual({
      type: "turn.metrics",
      inputTokens: 100,
      outputTokens: 12,
      cacheReadTokens: 30,
      cacheWriteTokens: 20,
      cacheHitPercent: 20,
    });
    expect(copilotMetricsFromPromptResult({ usage: {} })).toBeNull();
  });

  it("extracts session and current model and keeps active catalog model first", () => {
    const result = {
      sessionId: "copilot-session",
      models: {
        currentModelId: "claude-sonnet-5",
        availableModels: [
          { modelId: "auto", name: "Auto" },
          { modelId: "claude-sonnet-5", name: "Claude Sonnet 5", _meta: { copilotUsage: "1x" } },
          { modelId: "auto", name: "Auto duplicate" },
        ],
      },
    };
    expect(copilotSessionId(result)).toBe("copilot-session");
    expect(copilotCurrentModelId(result)).toBe("claude-sonnet-5");
    expect(modelsFromCopilotSession(result)).toEqual([
      { id: "copilot:claude-sonnet-5", harness: "copilot", name: "Claude Sonnet 5", nativeId: "claude-sonnet-5" },
      { id: "copilot:auto", harness: "copilot", name: "Auto", nativeId: "auto" },
    ]);
    expect(copilotCurrentModelId({ models: {} })).toBeUndefined();
    expect(copilotSessionId({ sessionId: " " })).toBeUndefined();
    expect(modelsFromCopilotSession({ models: { availableModels: [{ modelId: "auto", name: "Auto" }] } })).toEqual([
      { id: "copilot:auto", harness: "copilot", name: "Auto", nativeId: "auto" },
    ]);
  });

  it("promotes authentication errors but ignores debug, info, and unrelated stderr", () => {
    expect(copilotStderrAuthError("2026-09-26 [ERROR] auth: authentication failed")).toBe(`authentication failed\n\n${COPILOT_AUTH_HELP}`);
    expect(copilotStderrAuthError("[INFO] authentication failed")).toBeNull();
    expect(copilotStderrAuthError("[DEBUG] missing token")).toBeNull();
    expect(copilotStderrAuthError("[ERROR] network unavailable")).toBeNull();
  });
});
