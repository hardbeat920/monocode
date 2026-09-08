// TRANSPORT SEAM: see src/lib/transport/.
import { invoke } from "../transport";

export type StoredCursorToolCall = {
  toolCallId: string;
  toolName: string;
  args: unknown;
};

export function readStoredCursorToolCalls(
  sessionId: string,
  toolCallIds: string[],
): Promise<StoredCursorToolCall[]> {
  return invoke<StoredCursorToolCall[]>("cursor_tool_calls", {
    sessionId,
    toolCallIds,
  });
}
