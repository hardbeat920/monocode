import { invoke } from "@tauri-apps/api/core";

export type StoredCursorToolCall = {
  toolCallId: string;
  toolName: string;
  args: unknown;
};

export type StoredCursorSubagentRun = {
  toolCallId: string;
  agentId: string;
  revision: string;
  agentType?: string | null;
  model?: string | null;
  prompt?: string | null;
  steps: Array<{
    id: string;
    kind: "message" | "reasoning" | "tool";
    text: string;
    toolName?: string;
    args?: unknown;
    status?: string;
    output?: string;
  }>;
};

export function readStoredCursorSubagentRuns(
  sessionId: string,
  toolCallIds: string[],
  knownRevisions: Record<string, string> = {},
): Promise<StoredCursorSubagentRun[]> {
  return invoke("cursor_subagent_runs", {
    sessionId,
    toolCallIds,
    knownRevisions,
  });
}

export function readStoredCursorToolCalls(
  sessionId: string,
  toolCallIds: string[],
): Promise<StoredCursorToolCall[]> {
  return invoke<StoredCursorToolCall[]>("cursor_tool_calls", {
    sessionId,
    toolCallIds,
  });
}
