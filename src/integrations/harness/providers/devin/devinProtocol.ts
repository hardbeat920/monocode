import {
  promptBlocks,
  type PromptContentBlock,
} from "../../../../features/sessions/model/attachments";
import type { AgentModel } from "../../../../features/sessions/model/models";
import type {
  Attachment,
  RuntimeMode,
} from "../../../../features/sessions/model/session";
import {
  autoPermissionOption,
  asRecord,
  eventsFromAcpUpdate,
  extractModelConfigId,
  modelsFromSessionNew as modelsFromStandardAcpSession,
  permissionOptionId,
  permissionRequestFromAcp,
  readConfigOptions,
  resolveSettingConfigId,
  sessionIdFromResult,
  type SessionConfigOption,
} from "../antigravity/antigravityProtocol";

export {
  autoPermissionOption,
  asRecord,
  eventsFromAcpUpdate,
  extractModelConfigId,
  permissionOptionId,
  permissionRequestFromAcp,
  readConfigOptions,
  resolveSettingConfigId,
  sessionIdFromResult,
};
export type { SessionConfigOption };

export const DEVIN_CLIENT_INFO = {
  name: "windsurf",
  version: "1.110.1",
} as const;

export const DEVIN_CLIENT_CAPABILITIES = {
  fs: { readTextFile: false, writeTextFile: false },
  terminal: false,
  session: { configOptions: { boolean: {} } },
  _meta: { "cognition.ai/requestDiagnostics": true },
};

export type DevinModeId =
  | "ask"
  | "plan"
  | "accept-edits"
  | "smart"
  | "bypass";

export function devinModeId(
  runtimeMode: RuntimeMode,
  planning = false,
): DevinModeId {
  if (planning) return "plan";
  if (runtimeMode === "full-access") return "bypass";
  if (runtimeMode === "auto") return "smart";
  if (runtimeMode === "auto-accept-edits") return "accept-edits";
  return "ask";
}

export function devinPromptBlocks(
  text: string,
  attachments: Attachment[] = [],
): PromptContentBlock[] {
  return promptBlocks(text, attachments);
}

export function modelsFromDevinSession(raw: unknown): AgentModel[] {
  return modelsFromStandardAcpSession(raw).map((model) => {
    const nativeId =
      model.nativeId ?? model.id.replace(/^antigravity:/, "");
    return {
      ...model,
      id: `devin:${nativeId}`,
      harness: "devin" as const,
      nativeId,
    };
  });
}
