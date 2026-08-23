import type { AgentModel } from "../models";
import {
  asRecord as claudeAsRecord,
  assistantTextBlocks,
  assistantToolUses,
  contextUsedFromAssistant,
  isSubagentMessage,
  parseJsonLine as claudeParseJsonLine,
  previewFromTool,
  stringField as claudeStringField,
  toolKindFromName,
  toolResultsFromUserMessage,
  toolTitle,
} from "./claudeProtocol";

export { previewFromTool, toolKindFromName, toolTitle };

/**
 * Build amp CLI spawn args.
 *
 * `--stream-json-thinking` implies `--stream-json`, which `--stream-json-input`
 * requires. `--execute` keeps the process alive for the whole conversation
 * when paired with `--stream-json-input`.
 * `--dangerously-allow-all` disables permission prompts: Amp has no permission
 * channel over the stream, so monocode cannot act as the approval UI.
 */
export function buildAmpSpawnArgs(input: {
  mode?: string;
  fast?: boolean;
  threadId?: string;
}): string[] {
  const args: string[] = [];
  if (input.threadId) {
    args.push("threads", "continue", input.threadId);
  }
  args.push(
    "--execute",
    "--stream-json-thinking",
    "--stream-json-input",
    "--dangerously-allow-all",
  );
  if (input.mode) args.push("--mode", input.mode);
  if (input.fast) args.push("--fast");
  return args;
}

export function buildAmpUserMessage(
  text: string,
  attachments?: { kind: string; mimeType: string; data: string }[],
): Record<string, unknown> {
  const content: Array<Record<string, unknown>> = [];
  const trimmed = text.trim();
  if (trimmed) content.push({ type: "text", text: trimmed });
  for (const attachment of attachments ?? []) {
    if (attachment.kind === "image" && attachment.data) {
      content.push({
        type: "image",
        source: {
          type: "base64",
          media_type: attachment.mimeType,
          data: attachment.data,
        },
      });
    }
  }
  return {
    type: "user",
    message: {
      role: "user",
      content,
    },
  };
}

export function buildAmpSteerMessage(text: string): Record<string, unknown> {
  return {
    type: "user",
    steer: true,
    message: {
      role: "user",
      content: [{ type: "text", text: text.trim() }],
    },
  };
}

export function parseJsonLine(line: string): Record<string, unknown> | null {
  return claudeParseJsonLine(line);
}

export function asRecord(value: unknown): Record<string, unknown> | null {
  return claudeAsRecord(value);
}

export function stringField(
  rec: Record<string, unknown> | null | undefined,
  key: string,
): string | undefined {
  return claudeStringField(rec, key);
}

export function sessionIdFromInit(
  rec: Record<string, unknown>,
): string | undefined {
  if (stringField(rec, "type") !== "system") return undefined;
  if (stringField(rec, "subtype") !== "init") return undefined;
  return stringField(rec, "session_id");
}

export function isEndTurn(rec: Record<string, unknown>): boolean {
  const message = asRecord(rec.message);
  return stringField(message, "stop_reason") === "end_turn";
}

export function isErrorResult(rec: Record<string, unknown>): boolean {
  return (
    stringField(rec, "type") === "result" &&
    rec.is_error === true
  );
}

export function errorFromResult(rec: Record<string, unknown>): string {
  return (
    stringField(rec, "error") ??
    stringField(rec, "message") ??
    "Amp reported an error"
  );
}

export function errorFromSystem(rec: Record<string, unknown>): string | undefined {
  return stringField(rec, "error");
}

export function ampModeFromModel(model: string): string | undefined {
  const trimmed = model.trim();
  if (!trimmed) return undefined;
  const colon = trimmed.indexOf(":");
  const slug = colon >= 0 ? trimmed.slice(colon + 1) : trimmed;
  if (["low", "medium", "high", "ultra"].includes(slug)) return slug;
  return undefined;
}

export const AMP_MODELS: AgentModel[] = [
  {
    id: "amp:low",
    harness: "amp",
    name: "Low",
    nativeId: "low",
  },
  {
    id: "amp:medium",
    harness: "amp",
    name: "Medium",
    nativeId: "medium",
  },
  {
    id: "amp:high",
    harness: "amp",
    name: "High",
    nativeId: "high",
  },
  {
    id: "amp:ultra",
    harness: "amp",
    name: "Ultra",
    nativeId: "ultra",
  },
];

export function contextFromAssistant(
  rec: Record<string, unknown>,
): number | undefined {
  if (isSubagentMessage(rec)) return undefined;
  return contextUsedFromAssistant(rec);
}

export function textBlocksFromAssistant(rec: Record<string, unknown>): string[] {
  if (isSubagentMessage(rec)) return [];
  return assistantTextBlocks(rec);
}

export function toolUsesFromAssistant(
  rec: Record<string, unknown>,
): Array<{ id: string; name: string; input: Record<string, unknown> }> {
  if (isSubagentMessage(rec)) return [];
  return assistantToolUses(rec);
}

export function toolResultsFromUser(
  rec: Record<string, unknown>,
): Array<{ toolUseId: string; isError: boolean; text: string }> {
  return toolResultsFromUserMessage(rec);
}
