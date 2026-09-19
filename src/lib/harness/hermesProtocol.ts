import { promptBlocks, type PromptContentBlock } from "../attachments";
import type { AgentModel } from "../models";
import type { Attachment, RuntimeMode } from "../session";

export const HERMES_AUTH_HELP =
  "Configure Hermes with `hermes model`, then verify it with `hermes acp --check`.";

/**
 * Hermes writes ordinary provider-health warnings to stderr. Only promote
 * explicit authentication failures into the conversation; a broad match on
 * words such as `credential` also catches logger names like
 * `agent.credential_pool` and makes successful turns look broken.
 */
export function hermesStderrAuthError(line: string): string | null {
  const detail = line.trim();
  if (!detail || /\[(?:debug|info|warning)\]/i.test(detail)) return null;

  const message = detail.replace(
    /^\d{4}-\d{2}-\d{2}[^[]*\[(?:error|critical)\]\s*(?:[\w.]+:\s*)?/i,
    "",
  );
  if (
    !/(?:not authenticated|authentication (?:required|failed)|unauthorized|(?:missing|invalid|expired|revoked) (?:api key|credential|token)|no (?:llm )?provider (?:is )?configured|provider.+(?:not configured|requires authentication))/i.test(
      message,
    )
  ) {
    return null;
  }
  return `${message}\n\n${HERMES_AUTH_HELP}`;
}

/** Hermes accepts the standard ACP text, image, and resource-link blocks. */
export function hermesPromptBlocks(
  text: string,
  attachments: Attachment[] = [],
): PromptContentBlock[] {
  return promptBlocks(text, attachments);
}

/** Map MonoCode access levels to Hermes' edit-approval modes. */
export function hermesModeId(
  runtimeMode: RuntimeMode,
  planning = false,
): "default" | "accept_edits" | "dont_ask" {
  if (planning || runtimeMode === "supervised") return "default";
  if (runtimeMode === "auto-accept-edits") return "accept_edits";
  return "dont_ask";
}

export function hermesStartupError(error: unknown): Error {
  const detail = error instanceof Error ? error.message : String(error);
  if (/auth|credential|api key|provider|configure|setup/i.test(detail)) {
    return new Error(`${detail.trim()}\n\n${HERMES_AUTH_HELP}`);
  }
  if (/timed out/i.test(detail)) {
    return new Error(`Hermes Agent did not start. ${HERMES_AUTH_HELP}`);
  }
  return new Error(`Hermes Agent did not start. ${detail}`);
}

export function hermesSessionId(result: unknown): string | undefined {
  const rec = asRecord(result);
  const id = rec?.sessionId ?? rec?.session_id ?? rec?.id;
  return typeof id === "string" && id.trim() ? id.trim() : undefined;
}

export function hermesCurrentModelId(result: unknown): string | undefined {
  const rec = asRecord(result);
  const models = asRecord(rec?.models);
  const value = models?.currentModelId ?? models?.current_model_id;
  return typeof value === "string" && value.trim() ? value.trim() : undefined;
}

/** Read the standard ACP SessionModelState returned by Hermes session/new. */
export function modelsFromHermesSession(result: unknown): AgentModel[] {
  const rec = asRecord(result);
  const state = asRecord(rec?.models);
  const raw = state?.availableModels ?? state?.available_models;
  if (!Array.isArray(raw)) return [];

  const current = hermesCurrentModelId(result);
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
      id: `hermes:${nativeId}`,
      harness: "hermes",
      name: name || displayName(nativeId),
      nativeId,
    });
  }

  // The model active in Hermes is the right default for a newly selected
  // provider. setHarnessModels chooses the first model for generic catalogs.
  if (current) {
    const index = models.findIndex((model) => model.nativeId === current);
    if (index > 0) models.unshift(...models.splice(index, 1));
  }
  return models;
}

function displayName(nativeId: string): string {
  const slug = nativeId.includes(":")
    ? nativeId.slice(nativeId.lastIndexOf(":") + 1)
    : nativeId;
  return slug
    .replace(/[-_]+/g, " ")
    .replace(/\b\w/g, (char) => char.toUpperCase());
}

export function asRecord(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}
