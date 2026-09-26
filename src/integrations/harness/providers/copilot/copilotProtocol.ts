import type { AgentModel } from "../../../../features/sessions/model/models";
import type { RuntimeMode } from "../../../../features/sessions/model/session";
import type { HarnessEvent } from "../../core/types";
import { asRecord } from "../grok/grokProtocol";

export const COPILOT_AUTH_HELP =
  "GitHub Copilot is not signed in. Run `copilot login` in a terminal.";

export function copilotModeId(runtimeMode: RuntimeMode, planning = false): string {
  const mode = planning ? "plan" : runtimeMode === "full-access" ? "autopilot" : "agent";
  return `https://agentclientprotocol.com/protocol/session-modes#${mode}`;
}

export function copilotStderrAuthError(line: string): string | null {
  const detail = line.trim();
  if (!detail || /\[(?:debug|info|warning)\]/i.test(detail)) return null;
  const message = detail.replace(
    /^\d{4}-\d{2}-\d{2}[^[]*\[(?:error|critical)\]\s*(?:[\w.]+:\s*)?/i,
    "",
  );
  if (!/(?:not authenticated|authentication (?:required|failed)|unauthorized|(?:missing|invalid|expired|revoked) (?:credential|token)|(?:not signed in|login required|sign in required))/i.test(message)) return null;
  return `${message}\n\n${COPILOT_AUTH_HELP}`;
}

export function copilotStartupError(error: unknown): Error {
  const detail = error instanceof Error ? error.message : String(error);
  if (/auth|credential|token|login|sign in/i.test(detail)) {
    return new Error(`${detail.trim()}\n\n${COPILOT_AUTH_HELP}`);
  }
  if (/timed out/i.test(detail)) {
    return new Error(`Copilot CLI did not start. ${COPILOT_AUTH_HELP}`);
  }
  return new Error(`Copilot CLI did not start. ${detail}`);
}

export function copilotSessionId(result: unknown): string | undefined {
  const rec = asRecord(result);
  const id = rec?.sessionId ?? rec?.session_id ?? rec?.id;
  return typeof id === "string" && id.trim() ? id.trim() : undefined;
}

export function copilotCurrentModelId(result: unknown): string | undefined {
  const rec = asRecord(result);
  const models = asRecord(rec?.models);
  const value = models?.currentModelId ?? models?.current_model_id;
  return typeof value === "string" && value.trim() ? value.trim() : undefined;
}

export function modelsFromCopilotSession(result: unknown): AgentModel[] {
  const rec = asRecord(result);
  const state = asRecord(rec?.models);
  const raw = state?.availableModels ?? state?.available_models;
  if (!Array.isArray(raw)) return [];

  const current = copilotCurrentModelId(result);
  const seen = new Set<string>();
  const models: AgentModel[] = [];
  for (const item of raw) {
    const model = asRecord(item);
    if (!model) continue;
    const nativeId = String(
      model.modelId ?? model.model_id ?? model.value ?? model.id ?? "",
    ).trim();
    if (!nativeId || seen.has(nativeId)) continue;
    seen.add(nativeId);
    const name = String(model.name ?? model.title ?? nativeId).trim();
    models.push({
      id: `copilot:${nativeId}`,
      harness: "copilot",
      name: name || nativeId.replace(/[-_]+/g, " ").replace(/\b\w/g, (char) => char.toUpperCase()),
      nativeId,
    });
  }
  if (current) {
    const index = models.findIndex((model) => model.nativeId === current);
    if (index > 0) models.unshift(...models.splice(index, 1));
  }
  return models;
}

export function copilotMetricsFromPromptResult(
  result: unknown,
): Extract<HarnessEvent, { type: "turn.metrics" }> | null {
  const usage = asRecord(asRecord(result)?.usage);
  if (!usage) return null;
  const inputTokens = typeof usage.inputTokens === "number" ? usage.inputTokens : undefined;
  const outputTokens = typeof usage.outputTokens === "number" ? usage.outputTokens : undefined;
  const cacheReadTokens = typeof usage.cachedReadTokens === "number" ? usage.cachedReadTokens : undefined;
  const cacheWriteTokens = typeof usage.cachedWriteTokens === "number" ? usage.cachedWriteTokens : undefined;
  if (inputTokens == null && outputTokens == null && cacheReadTokens == null && cacheWriteTokens == null) return null;
  const cacheableInput = (inputTokens ?? 0) + (cacheReadTokens ?? 0) + (cacheWriteTokens ?? 0);
  return {
    type: "turn.metrics",
    ...(inputTokens != null ? { inputTokens } : {}),
    ...(outputTokens != null ? { outputTokens } : {}),
    ...(cacheReadTokens != null ? { cacheReadTokens } : {}),
    ...(cacheWriteTokens != null ? { cacheWriteTokens } : {}),
    ...((cacheReadTokens != null || cacheWriteTokens != null) && cacheableInput > 0
      ? { cacheHitPercent: ((cacheReadTokens ?? 0) / cacheableInput) * 100 }
      : {}),
  };
}
