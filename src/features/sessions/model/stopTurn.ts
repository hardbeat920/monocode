import { stopStreaming } from "../../../integrations/harness/core/apply";
import {
  buildDeterministicHandoff,
  completeHandoff,
  isPreparingHandoff,
} from "./handoff";
import type { Session } from "./session";
import { disarmUsageLimit } from "./usageLimit";

/** The session after the user presses Stop on its turn. */
export function stopSessionTurn(session: Session): Session {
  const stopped = stopStreaming(session);
  const completed = isPreparingHandoff(stopped)
    ? completeHandoff(stopped, buildDeterministicHandoff(stopped))
    : stopped;
  // Stop means stop: resume-at-reset must not start a turn later.
  const ready: Session = disarmUsageLimit({
    ...completed,
    worktreePreparing: undefined,
  });
  return ready.queuedMessages?.length
    ? { ...ready, queueStatus: "paused" }
    : ready;
}
