import { isMonoSession } from "../../features/monos/model/mono";
import { isHabitRun } from "../../features/monos/model/monoHabits";

export function canReuseAutomationSession(sessionId: string): boolean {
  return !isMonoSession(sessionId) && !isHabitRun(sessionId);
}
