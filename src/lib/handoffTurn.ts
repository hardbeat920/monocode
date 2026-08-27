import { buildOutgoingHandoffPrompt } from "./handoff";
import {
  cancelHarnessTurn,
  respondHarnessApproval,
  sendHarnessTurn,
} from "./harness/registry";
import type { HarnessId } from "./session";

const HANDOFF_TIMEOUT_MS = 45_000;

export async function requestOutgoingHandoff(input: {
  harness: HarnessId;
  sessionId: string;
  cwd: string;
  model: string;
  modelSettings?: Record<string, string>;
  userRequest: string;
}): Promise<string> {
  let brief = "";
  const timer = setTimeout(() => {
    void cancelHarnessTurn(input.harness, input.sessionId);
  }, HANDOFF_TIMEOUT_MS);
  try {
    await sendHarnessTurn({
      harness: input.harness,
      sessionId: input.sessionId,
      cwd: input.cwd,
      model: input.model,
      modelSettings: input.modelSettings,
      runtimeMode: "supervised",
      text: buildOutgoingHandoffPrompt(input.userRequest),
      onEvent: (event) => {
        if (event.type === "message.delta") {
          brief += event.text;
        }
        if (event.type === "approval.requested") {
          respondHarnessApproval(
            input.harness,
            input.sessionId,
            event.requestId,
            "deny",
          );
        }
      },
    });
  } catch {
    // Caller falls back to the deterministic packet.
  } finally {
    clearTimeout(timer);
  }
  return brief.trim();
}
