import { invoke } from "@tauri-apps/api/core";
import { hasHeadlessChildBackend } from "../../core/child";
import type { AntigravitySubagentName } from "./antigravityProtocol";

/** Subagent roles from Antigravity's local store; a remote host's store is not on this machine. */
export function antigravitySubagentNames(
  sessionId: string,
): Promise<AntigravitySubagentName[]> {
  if (hasHeadlessChildBackend()) return Promise.resolve([]);
  return invoke<AntigravitySubagentName[]>("antigravity_subagents", {
    sessionId,
  });
}
